# Contributing to Go Together

Thanks for helping make Go Together easier to play, learn from, and reuse.
Small, focused changes are the easiest to review.

A bug goes in an [issue](https://github.com/SKutai/go-together/issues/new/choose):
the bug report form asks for what a fix needs, and a blank issue is there for
an idea or a question. A suspected vulnerability does not go in an issue:
report it privately, as
[docs/SECURITY.md](docs/SECURITY.md#reporting-a-vulnerability) describes. If
you work with a coding agent, [AGENTS.md](AGENTS.md) is its short way into
this guide.

## Setup

1. Install the stable Rust toolchain with `rustup`.
2. Install the platform packages from the
   [Bevy setup guide](https://bevy.org/learn/quick-start/getting-started/setup/).
3. Clone the repository and run `cargo run`.

The checked-in toolchain configuration also installs
`wasm32-unknown-unknown`. To work on the HTML5 build, install the pinned builder
with `cargo install trunk --locked --version 0.21.14`, then run `trunk serve`.

On Debian or Ubuntu, CI installs:

```sh
sudo apt-get install g++ libasound2-dev libudev-dev libwayland-dev \
  libx11-dev libxkbcommon-dev pkg-config
```

The project uses the Rust 2024 edition and Bevy 0.20.0, which needs Rust 1.97.1
or newer; the stable toolchain above is newer than that.

## Small-change workflow

1. Fork the repository and create a descriptive branch from `main`.
2. Keep a pull request focused on one behavior or maintainability goal.
3. Add or update tests for behavior changes.
4. Update README or `docs/` when commands, rules, modules, controls, or
   persistence change.
5. Run the full check set before opening the pull request.
6. Explain observable changes, compatibility impact, and validation in the pull
   request description; a new pull request opens with a heading for each.

Avoid unrelated formatting or stylistic rewrites. Preserve current game
behavior unless the pull request explicitly proposes a design change.

A pull request is the only way a change reaches `main`; nobody pushes to it
directly, the owner included. Yours can be merged once both CI jobs have
passed on it and its branch is up to date with `main`, so bring `main` into
the branch again if it has moved on since you started. On your first pull
request here the jobs do not start until the owner has approved the run, so
two checks that sit waiting are expected and say nothing about your change.
Do not mark a commit `[skip ci]`, even one that touches documents alone: a
pull request whose newest commit skips CI has no passing jobs and cannot be
merged. The owner merges it once it has been read, with a merge commit and
not a squash: your commits arrive on `main` as you wrote them, which makes
each one's message worth writing for whoever reads the history later.

Design reasoning, invariants and measurements belong in `docs/` or in a doc
comment, where the next person to change that code will find them - not only
in the pull request description, which is read at review and seldom after.
A number written there is one that was measured, and carries its date; do
not write down one you have not measured.

An idea that is not settled belongs in neither place. The documents directly
under `docs/` describe the project as it is, and a claim in one of them is
meant to be true right now; a proposal filed beside them would cost them
that. Park it in [`docs/ideas/`](docs/ideas/README.md) instead, which holds
proposals, deferred decisions and approaches considered and rejected - and
which nothing may cite as evidence of how the game behaves. That folder is
for writing down an idea that has been discussed and parked; to raise one in
the first place, open an issue.

## Required checks

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

That list is everything CI can fail on, in the same configurations: the Bevy
client, the headless core (`--no-default-features`), and the `wasm32` build
each get one Clippy run with warnings denied. Clippy is a rustc wrapper and
reports the same compiler warnings, so there is no separate `cargo check`
beside any of them; `client` is the only feature, which makes `--all-features`
and the default feature set the same set. It is also the whole of what CI
runs on a commit: the advisory scan described at the end of this section lives
in its own weekly workflow and touches no push or pull request, so the list
above is the whole of what a green run asserts.

Because warnings are denied in the headless configuration too, "no unused
code" has to hold there separately: an item in a core module (`rules`,
`opening`, `match_settings`, `clock`, `authority`, `view`, `cue`, `replay`)
whose only callers are client code is dead code under
`--no-default-features`, and so is a `#[cfg(test)]` fixture that only a
client test uses. Gate the item - or the re-export path that reaches it - on
`client`, or move it beside its callers. See
[docs/TESTING.md](docs/TESTING.md) for the cases this has already caught.

The headless pair is not a cheaper substitute for the full list. `mod ui` is
gated on `client`, so `cargo test --locked --no-default-features` compiles no
UI code at all - every assertion over on-screen copy runs only in the default
configuration. A client-side change can therefore pass the headless commands
cleanly while breaking the tests written to catch it, which is the shape of
false green hardest to notice: the run is fast, green, and silent about what
it skipped. Run both.

That also makes the headless pair the only part of this list a machine without
Bevy's system packages can run. Install them (the README lists the set CI
uses, and `.github/workflows/ci.yml` installs the same one) before concluding
anything from a green headless run on a fresh container.

Run the two `cargo doc` forms above whenever you move a type or module between
files: a relative intra-doc link such as ``[`super::fusion`]`` breaks without
failing a plain `cargo doc`, which only warns about one in a public item and
does not read a private item at all, and grepping for the new path will not
find it because it never spells it out. The native form documents the default
features for the host, and the `wasm32` form covers the modules it cannot see
- `platform::browser_storage`, `platform::web`, and `BROWSER_REPLAY_LIMIT` in
`platform`.

Neither builds the headless documentation a `default-features = false`
consumer would read, where a link from a headless item to a `client`-only one
is unresolved. That configuration is not in the list above; if you add a doc
link from `replay`, `cue`, `authority`, `view`, `clock`, `rules`, `opening`,
or `match_settings` to anything behind `client`, check it with
`RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-default-features --no-deps --document-private-items`,
which should report nothing - name client-only items in plain backticks from
headless docs instead.

`-D warnings` denies every rustdoc lint that warns by default, not only the
broken-link ones:
write a URL in a doc comment as `<https://...>`, keep generics such as
`Vec<u8>` in backticks (rustdoc reads a bare one as an unclosed HTML tag), and
give every non-Rust fenced block in a `///` or `//!` comment a language label
such as `text` or `json`. An unlabeled fence is a doctest - `cargo test`
compiles and runs it in both configurations, and Clippy never sees it.
`src/replay/schema.rs` uses `compile_fail` doctests deliberately, to pin that
`Replay`'s fields stay private and that it gains no `Serialize` impl, so a
change there can fail `cargo test` through a doc comment.

If touching `Cargo.toml`'s dependency graph, also consider running `cargo tree
--duplicates` and, if available locally, `cargo audit`. CI runs `cargo audit`
too, but not on your commit: it lives in `.github/workflows/audit.yml`, which
runs weekly and on manual dispatch, and in which the scan and the
`cargo install` that precedes it are both `continue-on-error`, so neither can
turn a run red. It gates nothing, which means a green CI run is not a
statement that the advisory scan found anything - or even that it ran at all
this week. Read that workflow's log, dispatch it from the Actions tab, or run
`cargo audit` locally when that answer matters. See
[docs/SECURITY.md](docs/SECURITY.md#dependencies-and-automation).

Run `cargo run` when changing startup, assets, plugins, screen transitions, UI,
or audio. Confirm that the Home screen opens and that the terminal reports no
asset errors.

The package list in Setup above is enough to *build* the client but not to
*run* it, which is easy to miss because every one of the twelve required
checks passes without the difference. On a bare Debian or Ubuntu machine
`cargo run` needs two more: `libxkbcommon-x11-0`, without which startup
panics with "Library libxkbcommon-x11.so could not be loaded", and a Vulkan
driver such as `mesa-vulkan-drivers`, without which wgpu finds no adapter and
Bevy panics with "Unable to find a GPU!". Neither failure is about your
change. On a headless machine the window opens under `xvfb-run` with
llvmpipe's software rasterizer, which is slow but renders correctly and is
enough to check layout and read text.

Disk is the limit the `native` job has run into. A GitHub-hosted
`ubuntu-latest` runner keeps the workspace, Cargo's home and `/tmp` on one
volume. On the `ubuntu-24.04` image that volume started the job with
**13-14GB** free of 72GB (2026-10-27); on 2026-11-27 `ubuntu-latest` was the
`ubuntu-26.04` image, whose volume started it with 91GB free of 145GB. The
job's `Show free disk space` and `Show disk use` steps log those figures on
every run. Measured
on 2026-10-26 in an arm64 Ubuntu 24.04 container with Rust 1.99.1, the `dev`
profile's full debug information makes every binary that links Bevy over a
gigabyte: `target` reached 12.5GB once `cargo test --locked` had run and
14.6GB by the end of the job. The crate's doctests link one more such binary
into `/tmp` at the fullest moment of that, another 1.35GB. On the
`ubuntu-24.04` runner that was more than was left, and the link failed with
`ld terminated with signal 7 [Bus error]` - which is what a linker writing
through a memory map gets, instead of an error message, when the disk fills.

So the job builds with `CARGO_PROFILE_DEV_DEBUG: none`, which leaves local
builds alone. Measured the same way, it now peaks at **3.3GB**, the doctest
binary included (`line-tables-only` would have been 7.1GB), and on the
`ubuntu-24.04` runner the job ended with 9-11GB still free. Its cache holds
only the dependencies' build output and the downloaded crates, 2.5GB on disk
and 0.6GB as saved: the last steps remove this crate's own output before
saving, so a warm run rebuilds `go-together` and nothing else.

If disk runs short again, find what grew in those logged figures and shrink
what the job writes, rather than deleting the runner's preinstalled software
to make room - that only hides the growth until the next dependency spends the
margin again. About 70% of what is left is the dependencies' build output,
which every configuration needs. The headless pair adds about 0.15GB, so
neither `cargo clean` between configurations nor a job of its own for that pair
would recover much now. `CARGO_INCREMENTAL: 0` is already set.

`Install Trunk 0.21.14` in the `web` job is the one remaining blocking step
whose failure would not be a statement about this repository's code: a
from-source `cargo install` of a third-party tool that also inherits the
workflow-wide `RUSTFLAGS: -D warnings`, so a new warning anywhere in Trunk's
dependency tree would fail a required job. It has to block, because
`scripts/build-web.sh` is a required check and cannot run without Trunk, and it
has never failed. The hardening, if it ever does, is `env: RUSTFLAGS: ''` on
**that step only** - lifting it to the job would drop `-D warnings` from the
wasm Clippy and documentation steps that need it.

## Architecture and terminology

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) before moving systems between
features.

- `src/app/` composes plugins, owns `AppScreen`, and makes frame ordering
  explicit.
- `src/gameplay/` owns live Match state and applies semantic actions.
- `src/authority/` owns the authoritative, Bevy-independent `MatchState` that
  `gameplay` wraps - the command methods player intent reaches it through, the
  structured outcomes they answer in, and the Match clock (`values`,
  `outcomes`, `state`, `timing`). Its module documentation stays whole in
  `authority/mod.rs` rather than being divided among the submodules; see
  `docs/ARCHITECTURE.md`'s "Module ownership and dependency direction" section
  for why.
- `src/opening.rs` owns pure Round 0 selection and reveal rules.
- `src/rules/` is pure domain logic and must not depend on Bevy.
- `src/input/` maps devices to semantic actions.
- `src/platform/` adapts persistence and asset lookup to native filesystems or
  browser APIs.
- `src/settings/` owns persisted settings and migrations, split the way
  `src/replay/` is: `schema` is the on-disk wire format, `compat` the
  migrating loaders over it, `config` the live preference resources, and
  `store` the storage backend. A compatibility-sensitive change belongs in
  the first two and nowhere else.
- `src/replay/` owns the Replay schema, compatibility, validation, playback,
  and recording as pure, headless-core data (`schema`, `compat`,
  `validation`, `playback`, `recording`) - the crate's public API for
  loading, validating, inspecting, serializing, and playing back a Replay,
  with private fields and accessors so nothing outside the crate can hold a
  Replay that skipped `Replay::validate`. That last guarantee is why
  `MatchRecorder` is crate-private even though recording itself is core.
  `recording` is Bevy-free and compiles headlessly under `cfg(test)`, but its
  declaration is `#[cfg(any(feature = "client", test))]`: a headless non-test
  build has no caller for the crate-private `MatchRecorder` and would report
  it as dead code.
- `src/replay_library/` owns the saved-Replay collection the Library screen
  browses: `ReplayLibrary`, `ReplayCatalogEntry`, search/sort, and the
  "Unknown" fallback shown when the platform cannot format a Replay's local
  timestamp. It needs the `client` feature's storage and wall-clock boundary
  (`platform::ReplayStorage` and `platform::format_local_timestamp`). See
  `docs/ARCHITECTURE.md`'s "Headless core and Bevy client" section for the
  full split.
- `src/ui/` renders state and produces explicit intents; it does not decide
  rules.
- `src/architecture.rs` is test-only and holds the crate to the two claims
  above it - which top-level module may depend on which, and which pairs are
  allowed to depend on each other - plus the printable-ASCII rule for
  in-game text, the rule that only `src/presentation.rs` makes a color, and
  the 14px text-size floor, all under
  [UI and accessibility](#ui-and-accessibility), and
  provides `sources_in`/`source_file`, which read the crate's own source at
  test time: the `ui` tests walk whole directories asserting that retired
  phrasing is gone, and `replay::measurement` reads `platform/mod.rs` to
  check a mirrored `wasm32`-only constant. Read [Tests that read the crate's
  own source](docs/TESTING.md#tests-that-read-the-crates-own-source)
  before rewording UI copy: those scans read text, so a forbidden phrase in a
  comment fails the test too. `ALLOWED_EDGES` is checked in both directions -
  adding a dependency it does not permit fails `cargo test`, naming the file
  that introduced it, and so does removing the last use of one it still
  lists, which is a stale entry to delete rather than a permission to keep.
  A new top-level module needs a row there even if it depends on nothing, and
  a new mutual dependency additionally needs an entry in `DECLARED_CYCLES`
  saying why it is allowed. The table in `docs/ARCHITECTURE.md`'s "Module
  ownership and dependency direction" section deliberately does not repeat
  the exact edge list, so it needs no edit for an edge that fits the existing
  "Depends toward" summary.
- `src/contrast.rs` is test-only too, and holds the colors in
  `src/presentation.rs` to the contrast floor under
  [UI and accessibility](#ui-and-accessibility). It is a table of which
  color is drawn on which, so a new color, or an old one drawn somewhere
  new, needs a row there.
- `src/audio.rs` receives semantic cues; gameplay should not spawn audio
  entities directly.

Use the player-facing terms **Match**, **Replay**, **Select**, and **Teleport
Marker** consistently. Black is Player 1 on the right; White is Player 2 on the
left.

## Adding a Bevy system or plugin

1. Put the system in the feature that owns the state it changes.
2. Prefer a feature `Plugin` to registering systems from an unrelated module.
3. Add the system to the appropriate `AppSystems` stage, or add a clearly named
   stage when a new ordering boundary is necessary.
4. Use `run_if(in_state(...))` for screen-specific work.
5. On screen entry, reset resources before spawning entities by chaining
   `prepare_*` then `spawn_*`.
6. Tag every independently spawned screen root with Bevy's built-in
   `DespawnOnExit(that screen)` at the exact call that spawns it, and keep its
   descendants attached so they despawn recursively with it. This alone is
   enough; there is no per-screen cleanup system to register. See
   `docs/ARCHITECTURE.md`'s "Screen, modal, and persistent entity ownership"
   section for the four ownership lifetimes - including the one modal that
   is application-persistent because two screens show it - and the one case
   (`despawn_screen_entities_now`) that needs an immediate, non-state-transition
   despawn instead.
7. Group large Bevy parameter lists with a cohesive `SystemParam` instead of
   suppressing Clippy.
8. Add a small ECS test when ordering, cleanup, or a state transition matters.

Do not hide ordering assumptions in distant `.before()`/`.after()` chains when
membership in an `AppSystems` set, or an edge declared in
`configure_app_systems` (`src/app/mod.rs`), can express them. `AppSystems` is a
small dependency graph of real data dependencies, not one blanket pipeline.

## Changing persisted data

Settings schemas v1-v6 and Replay schemas v1-v6 are compatibility contracts.
Existing users must not lose controls, preferences, Replay labels, Match IDs,
events, or filenames.

When a format must change:

1. Add a new schema version; never reinterpret an existing version.
2. Keep old serde field and enum names loadable.
3. Add an explicit migration or best-effort compatibility path.
4. Preserve unknown/corrupt-file isolation behavior.
5. Keep writes atomic.
6. Add fixture-style tests for every supported older version and the new
   round-trip format. For Replays that includes a new literal file under
   `tests/fixtures/replays/` (its README says how); every older file there
   must keep loading and re-serializing byte-for-byte.
7. Run storage-backend contract tests with the native mock backend and keep the
   `wasm32-unknown-unknown` build compiling.
8. Update README, architecture, and testing documentation.

Moving a Rust type does not change JSON by itself, but changing serde names,
defaults, validation, board cell count, or event timing does.

One kind of exception to rules 1 and 2 has been taken, twice, and it is the
only kind that should be: a key name withdrawn because the interface reserved
the key for itself. `f1` left `KeyboardKey` when F1 became the Help key, and
`f11` when F11 became the fullscreen key, so a settings file naming either
loses its keyboard section to defaults with a warning. A file naming `escape`
has always failed to parse, since that key never had a name; until the
keyboard section was made recoverable, that failure cost the whole file. No
schema version could have expressed a withdrawal - a key no version may bind
is not a schema - and the per-section isolation of rule 4 is what makes the
cost one section rather than the file.

Native storage keeps completed Replays until manual deletion. Browser storage
keeps exactly the newest three completed Replays and may reject writes because
of quota or private-mode policy. Preserve those platform contracts unless a
change explicitly includes migration, failure-path tests, and documentation.
Browser values are attacker-controlled input just like native JSON files.

## Web shell and assets

`index.html`, `Trunk.toml`, and `web/` form a deliberately small shell around the
Bevy canvas. Keep scripts and styles external and local, preserve
`public_url = "./"`, and do not add CDNs, analytics, a PWA manifest, or a service
worker. `scripts/build-web.sh` must leave a self-contained `dist/` with
`index.html`, root JavaScript/WebAssembly, `assets/`, `web/`, and a copy of
`LICENSE` (`scripts/verify_web_build.py` checks for each of these); it must
never upload or publish. No file in `dist/` may hold a path of the machine
that built it, which the same verification checks: see
[If the check reports a path](docs/DISTRIBUTION.md#if-the-check-reports-a-path)
if it reports one.

Native builds resolve `assets/` beside the executable first. Only a debug build
then falls back to `assets/` under the current working directory, which is what
`cargo run` relies on; a packaged release build keeps the executable-adjacent
path even when that directory is missing, and uses the relative `assets` path
only when the executable itself cannot be located (see
`platform::native::asset_root`). Test both cases when changing asset lookup.
The web build copies the same committed assets.

## Updating generated audio

The committed marimba-note WAV files and Round 0 duplicate-selection clack are
original project assets generated by `tools/generate_marimba.py`.

```sh
python3 tools/generate_marimba.py
python3 tools/generate_marimba.py --check
cargo test audio::tests::generated_wavs_are_mono_pcm_click_free_and_unclipped
```

Change synthesis code and generated files in the same pull request. Do not add
downloaded samples or other assets without clear origin and license
documentation. Keep the semantic cue vocabulary in `src/cue.rs` - it is
headless core, and a `GameCue` variant's `snake_case` name is part of the
Replay format from schema v3 on, so variants are never renamed or reordered -
and the mapping from cues to notes in `src/audio.rs`; Replays store semantic
Game cues rather than raw sample paths.

## Testing expectations

- Put pure rule and data-format tests beside their owning module.
- Use a minimal Bevy `App` for system ordering, injected input, or entity
  cleanup.
- Use temporary persistence roots; never read or overwrite a contributor's
  real application data in tests.
- Test both default and remapped inputs when changing input behavior.
- Add compatibility tests for every persistence change.
- Exercise the platform storage contract through a mock backend in native tests;
  browser-only code must also compile for `wasm32-unknown-unknown`.
- Add a regression test before fixing a subtle timing, gray-stone, ko, hold, or
  Replay-seeking bug.
- When adding or changing a board/round-resolution invariant, consider whether
  a property test in `src/rules/properties.rs` covers it more broadly than one
  example case would, alongside (not instead of) an example test pinning the
  specific case you were fixing.

See [docs/TESTING.md](docs/TESTING.md) for test helpers and examples.

## UI and accessibility

Bevy's bundled fallback font does not support every decorative glyph. Keep
in-game text printable ASCII unless a bundled font is deliberately added and
tested on every screen. Prefer words and ordinary punctuation over icons.
`cargo test` enforces this: `production_string_literals_are_printable_ascii`
in `src/architecture.rs` reads every string literal in production code, in
every module and not only `src/ui/`, and fails naming the file, line, and
code point of anything outside printable ASCII - an escaped `\u{2014}` as
much as a bare em dash. A newline is allowed, being layout rather than a
glyph, so multi-line copy needs no special handling - but a tab is not.
Doc comments and `#[cfg(test)]` fixtures are exempt, so a test that types
an accented letter to check it is refused needs no allowlist entry;
`char` literals are not read at all. The test's doc comment says exactly
where that line is drawn.

Write in-game text as though the Match is being played in good faith. Where
a rule could be described by the way it can be abused or by the way it is
meant to be used, use the second: the end gesture is an agreement between
two players, even though one player at a shared keyboard can reach both
Select keys. This is a deliberate choice of the project owner's, made
2026-10-07, and it is about tone rather than accuracy - do not state
something false to be warm, but do not reach for the adversarial reading of
a cooperative game.

Check text at the smallest window, 1040x600, at the 1040x900 window the game
opens at, and at a large one such as 1920x1080: every screen is laid out for
any window from the smallest up, and changes arrangement between those sizes.
The real-layout tests already run at the sizes `NAMED_WINDOW_SIZES` names,
and the capture probe in `docs/TESTING.md` shows a screen at any size. Keep
labels readable, centered, and within their panels. Markdown documentation is
not constrained by the in-game font, but plain text remains the most portable
choice.

No text in the game is smaller than **14px**. That is a floor, not a default:
secondary copy - a description, a unit, a Match ID, a validation error - is
set at 14px and made secondary by its color and its place, never by being
smaller. Until 2026-10-07 the smallest text in the game was an 11px
validation error, set smaller than the description it explained, to a
player who had just made a mistake. Raising text can wrap a line that used
to fit, so measure the screen afterwards rather than assuming; `docs/TESTING.md`
carries the probe that does it. The floor is in the game's own pixels: a
window smaller than the smallest one the screens are laid out for - a browser
page under 1040x600, or fullscreen on a screen smaller than that - is drawn
as that smallest window scaled down to fit, which is the one place text falls
below 14px, and is recorded in `docs/ideas/accessibility-findings.md`.

`cargo test` enforces the floor: `no_text_is_set_below_the_floor` in
`src/architecture.rs` reads every `FontSize::Px` in production code and
fails on any size under 14px. Write a size as a number, or as a constant
whose value is one, where it is set - `FontSize::Px(14.0)`,
`FontSize::Px(LABEL_SIZE)` - because the scan follows a name only to its
declaration, and a size it cannot read fails as surely as a small one. A
helper that takes a size takes a `FontSize`, so the caller writes it. Other
units, `TextFont::from_font_size` and `12.0.into()` are reported too.

Do not encode meaning by color alone, and hold every color to a contrast
floor. The floor is WCAG 2.1 level AA, measured with its relative-luminance
formula:

- **4.5:1** for text;
- **3:1** for text of 24px or larger. WCAG also counts bold text from 18.66px
  as large, but the game sets every label in one weight, so size is the only
  way text qualifies; and
- **3:1** for anything that is not text and carries meaning: a control's
  border, a stone's edge, a bar's fill, two fills a player has to tell apart.

Decoration owes nothing, and neither does the frame of a control that is
switched off, exactly as in WCAG - though a disabled control's label is kept
readable anyway, because it says what is unavailable.

"Not by color alone" is the same rule seen from the other side, and the
numbers are what make it checkable: **two colors under 3:1 apart are one
color to a player who cannot see hue.** An orange border beside a blue one is
1.44:1, so marking a state with it is marking it by color alone, however
different the two look. A second channel is a word, a glyph, a shape, a size
or a position.

`cargo test` enforces the floor, in two halves:

- `src/contrast.rs` holds a table of every pairing that occurs on screen -
  this foreground, on this background, as text of this size or as a mark -
  computes each ratio from the real constants, and fails on any below its
  floor. Every `Color` constant in `src/presentation.rs`, and every function
  there that builds a color, must appear in at least one row, so a color
  cannot be added without saying where it is drawn. Translucent colors are
  composited over what is beneath them first, in linear light, which is how
  the renderer blends.
- `colours_are_constructed_only_in_presentation` in `src/architecture.rs`
  is what makes the first half mean anything: no other file may write a
  `Color::srgb(...)` literal, use a named color such as `TextColor::WHITE`,
  or derive a color with `with_alpha`, `set_alpha` and the like.
  `Color::NONE` is allowed; it is the absence of one. The scan reads what is
  written, so a color that arrives unwritten - a `Text` left to its default
  white - is the one thing it cannot catch; give every `Text` a `TextColor`
  from `presentation.rs`.

A pairing that cannot meet the floor is never just left out of the table. It
goes in the table's list of exemptions with the reason it falls short and one
of three accounts of how the player gets the meaning anyway: the second
channel that carries it, named; a statement that it is decoration or part of
an inactive control; or the plain admission that nothing carries it yet and
the owner has to decide what should. That last kind is counted, and the count
is pinned - at zero, since 2026-10-04 - so the debt cannot grow without
someone changing a number on purpose. An exemption whose pairing now passes fails the
test too - fixing a color means moving its row, so the list of exemptions
stays a true list of what is still wrong.

What the test cannot check is whether a row's claim about *where* a color is
drawn is true. Changing how a screen nests its panels, or what a control is
drawn on, is a reason to re-read the rows that name it.

Motion is held to two rules, implemented in `src/app/motion.rs` and
reasoned about in `docs/ARCHITECTURE.md`'s "Motion, and what a capture
shows". First, **motion is reducible**: anything that moves, scales, fades
or pulses reads `Motion::reduced()` and, when it is true, changes in a
single step instead. The player's Reduce Motion preference and the
browser's `prefers-reduced-motion` setting are the two sources, and either
is enough. Second, **the information survives with motion off**: design the
static form - a mark, a count, a word - first, as the thing the player is
owed, and build the animation on top of it. An animation that is the only
carrier of a change has moved a defect between channels, not fixed it. The
capture indication in `src/ui/match_screen/capture.rs` is the pattern, and
the Round 0 dot rows in `src/ui/match_screen/opening_dots.rs` are the case
where no end state can carry the change, so the mark is something else on
screen - there, the row's dividing line drawn as a cross. Where a sound
records a game fact, as the Round 0 clack does in the Replay, time the
animation to meet the sound rather than moving the sound.
Keep any new motion short: two seconds was the owner's ceiling for a capture
and was treated as far too long. The two bars below the board and the clock
texts read neither rule, because they are readouts rather than animations:
their motion is the measurement itself, the draining hold bar included.
`src/app/motion.rs`'s module doc says why that exempts them, and is the
place to argue with before exempting anything else.

For web changes, also test a narrow iframe and fullscreen. Keyboard play must
remain usable, pointer or focused-key interaction must start and focus the
canvas, and Web Audio initialization must happen while that trusted gesture's
activation lasts, which in Safari is about five seconds. The module must be
instantiated asynchronously, because Chromium refuses a synchronous one of this
size. Test the start in Chrome, Firefox, and Safari: the
[distribution guide](docs/DISTRIBUTION.md#starting-in-a-browser) says why each
differs. Browser gamepad behavior is supported only where the browser and
device provide a standard mapping.

By contributing, you agree that your contribution may be distributed under the
repository's [MIT License](LICENSE). The repository code and assets, including
the original generated WAV files, share that license. Read the
[distribution guide](docs/DISTRIBUTION.md) and
[security notes](docs/SECURITY.md) before preparing a release.
