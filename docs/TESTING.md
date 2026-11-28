# Testing

Go Together keeps tests beside the code they protect. Pure domain tests run
without a renderer or window; ECS integration tests build the smallest Bevy
`App` needed for a system pipeline.

## Commands

Run the release check set from the repository root:

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

Every configuration is linted the same way, with warnings denied: the Bevy
client, the headless core alone (`--no-default-features`: no Bevy, no
window), and the `wasm32` build. The two native configurations are tested the
same way too; the `wasm32` build has no test runner in this check set - it
gets Clippy, `cargo doc`, and the Trunk build, and its browser-only code is
covered natively through the mock backends and items gated
`cfg(any(target_arch = "wasm32", test))`. Clippy is a rustc wrapper and
reports the same compiler warnings, so no `cargo check` runs beside a Clippy
run over the same targets and features.

"No unused code" therefore holds in each configuration separately, which
constrains where an item may live: a helper only the client calls has to be
behind `client` rather than sitting in a headless module, or the headless
build reports it as dead. `match_settings`' text-parsing helpers
(`parse_digits_clamped`, `duration_parts`, and their siblings) are gated for
exactly that reason - they turn a player's typed input on the Settings screen
into a validated value, and a headless consumer has no text input. The
contrast worth keeping in mind is `authority`: its clock arithmetic looked
client-only the same way, but that was a missing headless capability rather
than a client concern, and it was resolved by giving `MatchState` its own
public `advance_clock` instead of by gating it. Replay's loading, validation,
and playback layer is public API, reachable headlessly, and reported clean.

The same rule applies to *test* helpers, and that is worth knowing before
adding one: a fixture in `replay::test_support` that no headless test uses
is dead code in the `--no-default-features` build even though it is
`#[cfg(test)]`. Two of them (`assert_storage_schema` and `replay_with_id`)
were in that position when `replay_library` was split out of `replay` - the
first gained a headless caller in `replay::compat`, the second moved to
`replay_library::test_support` beside its only user. Put a fixture where
its callers are.

All of `replay`'s in-module tests run in both configurations. See
`docs/ARCHITECTURE.md`'s "Headless core and Bevy client" section for the
boundary this rests on, including what used to keep them behind `client`
and why the mock storage backends live in `replay_library` rather than
beside the fixtures in `replay::test_support`.

Useful focused commands include:

```sh
cargo test rules::
cargo test rules::properties
cargo test authority::
cargo test opening::tests
cargo test input::controller::
cargo test settings::
cargo test replay::
cargo test --no-default-features replay::
cargo test replay_library::
cargo test --no-default-features --test headless_replay
cargo test replay::compat::tests::checked_in_fixtures_match_their_builders
cargo test gameplay::countdown::tests
cargo test gameplay::holds::tests
cargo test ui::replay
cargo test ui::help
cargo test audio::tests::generated_wavs_are_mono_pcm_click_free_and_unclipped
```

Use `cargo test -- --list` to see exact test paths after modules move.

### Several worktrees, one target directory

If you run this crate's tests from more than one git worktree at a time -
a second branch under review, a comparison against another branch -
give each worktree its own `CARGO_TARGET_DIR`, or make its build hash
distinct. Sharing one target directory is not safe here: the library test
binary gets the same file name in every worktree, and cargo's freshness check
can then run a *different* worktree's binary than the one you just built.

This has already produced two false results in this repository, on 2026-09-26:
one run that passed with mutations in place, and one that failed carrying
another worktree's instrumentation. Neither looked wrong at the time, which is
what makes it worth a paragraph.

A distinct hash without rebuilding Bevy from scratch, one `N` per worktree:

```sh
cargo test --config 'profile.dev.package.go-together.codegen-units=3'
```

A uniquely named test in each worktree - one that only exists there - proves
whose binary actually ran. `cargo clean -p go-together` in the checkout you
intend to trust also settles it, and is worth doing before a final check-set
run that several worktrees have been building alongside.

## Example tests, property tests, and fuzzing

Three layers, each covering what the others do not:

- **Example tests** (the large majority of the suite) pin exact, documented
  behavior for a specific, often hand-picked board shape or event sequence -
  "this exact zero-liberty merge scores this exact way." They are also where
  every minimized property-test failure becomes a permanent regression case
  (see below).
- **Property tests** (`src/rules/properties.rs`, using
  [`proptest`](https://docs.rs/proptest)) check that a *general* invariant
  holds across many generated inputs at once - "no board shape ever lets an
  ordinary move create a `Gray` cell" - rather than pinning one shape. They
  generate only through `Board`'s own public API (`get`/`set`/`apply_move`/
  `apply_cleanup`), never by constructing a `Board` from raw cells, so a
  property test can never rely on access an ordinary caller does not have.
  Run the whole layer with `cargo test rules::properties`; it runs as part of
  the ordinary `cargo test` suite. Both `proptest!` blocks pin 256 cases per
  property with `ProptestConfig::with_cases(256)` - enough to be useful, low
  enough to stay fast for routine development. Because that count is set in
  source, `PROPTEST_CASES` is ignored; edit those two lines for a longer
  local run. The seed is not pinned: proptest's default is a fresh one drawn
  from the OS on every run, so re-running the same command does not reproduce
  a CI failure. What makes a failure reproducible is the minimized input
  proptest reports, which is exactly what the named example test below pins.
  (`PROPTEST_RNG_SEED` is still honoured, since only `cases` is overridden,
  so a seed captured from a run can be replayed.)

  What the layer currently covers, at three levels:

  - *Board mechanics*: `apply_move`/`apply_cleanup` determinism, clean
    out-of-bounds rejection, an unchanged board after a rejected move, the
    two documented placement outcomes (stone stands or fully self-removes),
    `Gray` never created by an ordinary move and never converted or captured
    by any board operation, cleanup never touching the acting player's own
    stone or colour, and capture accounting - every reported removal really
    left the board, the captured and self-removed sets are disjoint, and no
    cell went empty *without* being reported. That last conservation clause
    is the point of the capture property: those counts become
    `capture_credits`, then `capture_totals`, then the score and the Point
    Target end condition, where a miscount is a wrong result rather than a
    crash.
  - *Round resolution*: argument-order independence for two simultaneous
    different points, a single lock never reporting a two-player resolution
    kind, and positional repetition - a repeated position reports
    `repetition`, resolves as `KoGray`, credits no captures, and Grays every
    attempted point that was empty before the round.
  - *Scoring and persistence*: `fill_territory` is idempotent and never
    alters an already-occupied cell, and `Board`'s hand-written serde
    implementations round-trip an arbitrary board exactly - the persisted
    Replay compatibility surface that example tests otherwise cover only for
    hand-picked shapes.

  Two things about the generators are worth knowing before adding a
  property. First, `arbitrary_board()` deliberately reaches positions no real
  match could, which is right for the robustness properties but too sparse
  for anything about captures: it produces no enclosed group at all over 256
  cases, so a capture property fed only by it passes regardless of what it
  asserts. `crowded_board()` packs the same kind of placements into a small
  region, and properties about removals draw from both. Second, positional
  repetition cannot be generated - a random history would essentially never
  hold the exact board a random round produces - so that property constructs
  its precondition by resolving the round once and seeding the result back
  into the history, generating only the board and the selections.
- **Fuzzing** (evaluated, not currently set up - see below) would target
  parsers that accept untrusted bytes directly, which is a different concern
  from either of the above: not "is this invariant true for well-formed
  inputs" but "can *arbitrary* bytes, well-formed or not, cause a panic,
  unbounded allocation, or a hang."

`proptest-regressions/` is deliberately not tracked (see `.gitignore`).
Proptest writes the seed of a failing case there and replays it on later runs,
and upstream convention is to commit it - but this repository's permanent
record of a failure is the named example test described below, not a seed
file. Two mechanisms for "remember this failure" means one of them rots, and
the seed file is also written during ordinary work whenever a property is made
to fail on purpose - negating an assertion to check the property is not
vacuous, for instance. Keep the named example test; let the seed file be
scratch.

When a property test finds a real bug, fix the implementation if it violates
a documented rule, then add the shrunk minimal case proptest reports as a
permanent, named example test - do not just let the property test itself
stand in for that regression coverage, since a future refactor could
accidentally weaken the property back to passing without ever re-exercising
that exact minimized case. If the documented rule itself is ambiguous about
what "correct" means for the failing case, treat that as a design question
to raise, not a property to quietly weaken until it passes.

**Deliberately not treated as invariants** - property tests must not assert
these, because the documented rules intentionally allow them (see
`docs/GAME_RULES.md` and the existing example tests before adding any new
board/resolution property):

- A surviving group is not guaranteed to have a liberty: an ordinary
  simultaneous merge conflict can leave a zero-liberty group on the board
  until a later cleanup action removes it.
- Suicide/self-removal is a permitted, intentional strategy, not an error
  case to reject.
- Repetition-to-Gray is intentionally exploitable, not a bug to prevent.
- Dead, zero-liberty stones still on the board at Match end may still count
  under the implemented scoring rules.

**Fuzzing evaluation.** Good candidate targets, if added later, are parsers
that decode untrusted bytes directly: Settings document deserialization
(`ControlsConfig::from_json`), Replay deserialization/validation
(`Replay::from_json`, which includes the v6 board-compaction resolution
pass), and the two Replay catalog parsers -
`platform::browser_storage::BrowserReplayCatalog::from_json`, compiled only
for `wasm32` and under `cfg(test)`, and
`platform::native::NativeReplayStorage::read_catalog_cache`. Each should
assert only that arbitrary bytes cannot panic, allocate unboundedly, index out
of bounds, or hang - not that they produce any particular `Ok`/`Err` split.
This project does not currently check in a `cargo-fuzz` target: it requires a
nightly toolchain and the `cargo-fuzz` subcommand, neither of which should
become a required part of routine `cargo test`, and open-ended fuzzing does
not belong in ordinary PR CI regardless of toolchain availability. A
contributor with the nightly toolchain and `cargo install cargo-fuzz`
available can still fuzz these boundaries manually.
`go_together::replay::Replay::from_json` is part
of the crate's headless public API (see `src/lib.rs`'s module docs), so a
`cargo-fuzz` target - an ordinary external crate depending on `go_together`
as a library, with `default-features = false` - calls it directly.
`ControlsConfig::from_json` and both catalog parsers are crate-private inside
client-only modules, so fuzzing *those* boundaries would still need a
`fuzzing`-style Cargo feature that `#[cfg]`-exposes a thin `pub` wrapper for
fuzz builds only, the standard pattern for fuzzing an internal API without
widening it for every other consumer - and the browser catalog parser cannot
be reached from a native `cargo-fuzz` build at all:

```sh
cargo +nightly install cargo-fuzz
cargo +nightly fuzz init
cargo +nightly fuzz add replay_from_json
# fuzz/fuzz_targets/replay_from_json.rs:
#   fuzz_target!(|data: &[u8]| {
#       if let Ok(text) = std::str::from_utf8(data) {
#           let _ = go_together::replay::Replay::from_json(text);
#       }
#   });
cargo +nightly fuzz run replay_from_json
```

If this project adopts a checked-in fuzz target in the future, add any
`fuzzing`-gated `pub` wrappers a client-only parser needs, keep the target
itself out of the required PR workflow (a separate, optional, possibly
scheduled CI job at most), and document the exact run command here.

## Test organization

| Area | Style | What it protects |
| --- | --- | --- |
| `opening.rs` | Pure unit tests | Three-choice uniqueness/FIFO behavior, validated serialization, reveal ownership |
| `rules/` | Pure unit + property tests | Captures and capture accounting, suicide, timing, gray outcomes, ko/repetition, territory, score, board serde |
| `authority/` | Pure unit tests over `MatchState`, no Bevy `App` | Command methods and structured outcomes (`try_lock`, ordinary round resolution), `advance_clock` stepping the Round 0 countdown one stage at a time, match time winning a simultaneous round deadline, Round 0 reveal on match-time expiry, the Replay-duration budget cap, and clone/restore of `MatchProgress` |
| `view.rs` | Pure unit tests | Every `PublicMatchView`/`PlayerMatchView` field agrees with its authoritative source; pending selections, Round 0 points, cursors outside play, and Teleport Markers stay private to their owner |
| `input/` | Pure + small ECS tests | Defaults, remapping, assignment, hysteresis, semantic fusion |
| `settings/` | JSON fixtures + temporary files | Schemas v1-v6, partial recovery, atomic round trips; `compat`'s tests are where a per-section fallback is pinned, `store`'s where a failing backend is |
| `replay/` | Pure unit tests, run in both build configurations | Schemas v1-v6, Round 0/rule validation, recording, playback, silent seek, board compaction; `compat`'s guard test pins `tests/fixtures/replays/` to its builders |
| `replay_library/` | Mock backends + temporary files | Sorting and search, persistence round trips, corrupt-file isolation, browser retention and eviction, quota failure |
| `tests/` | Integration tests over the public API only, with and without `client` | `headless_match.rs`: a Match driven through `authority`; `headless_replay.rs`: every schema version's literal file under `tests/fixtures/replays/` loaded, validated, inspected, re-serialized byte-for-byte, and played back |
| `platform/` | The real native adapter under temporary roots, plus the browser catalog codec compiled natively; the mock-backend contract tests sit with the ports' consumers in `replay_library/` and `settings/store.rs` | Catalog cache rebuild and corruption isolation, symlink refusal, per-schema rename, asset resolution, browser record bounds and UUID-keyed deletes |
| `gameplay/` | Small ECS apps | Countdown gates, Match clock, actions, holds, recording |
| `ui/` | Helpers + ECS apps | Text editing, layouts, navigation intents, cleanup, Replay visuals, the capture indication's marks and timeline (including a round resolving mid-animation) |
| `app/` | Small ECS apps + `capture_log_events` | `DespawnOnExit` cleanup across repeated screen visits, a modal blocking the Match clock the same frame, systems gated to an inactive screen never running, and the startup split: a generic `Diagnostic` for a failed settings load plus its technical detail reaching a real `warn!` from `startup_log`'s `Startup` system |
| `audio.rs` | Pure + ECS + asset checks | Cue meaning, button transitions, gain safety, WAV format |
| `architecture.rs` | Reads the crate's own source | Which top-level module may depend on which, which pairs may depend on each other, the directory walks the `ui` negative assertions scan, the printable-ASCII rule for every string literal in production code, and the rule that no file but `presentation.rs` constructs a colour |
| `contrast.rs` | A table of colour pairings, checked against the real constants | The WCAG AA contrast floor for every colour in `presentation.rs` on each background it is drawn on, the exemptions and their reasons, and - by reading `presentation.rs` - that no colour constant is missing from the table |

Tests may access private implementation details through their owning module.
This keeps production visibility narrow without creating a public test API.

### Tests that read the crate's own source

A few tests treat this crate's source files as data. Six kinds exist, all
reached through [`src/architecture.rs`](../src/architecture.rs):

- **Negative assertions** - "this phrasing is gone from the code" - held by
  `delete_dialog_omits_the_removed_irreversible_warning` and
  `replay_library_omits_removed_helper_copy` (`src/ui/replay/library/`),
  `replay_library_has_no_routine_success_narration` (which scans all of
  `src/replay/` and `src/replay_library/` rather than a screen),
  `hold_progress_track_has_bright_fixed_outline_and_center_out_geometry`,
  `timer_and_round_preferences_cover_all_hud_combinations` and
  `live_match_ui_has_no_contextual_alert_strip` (`src/ui/match_screen/` plus
  `src/ui/hud.rs`), and `replay_viewer_has_no_persistent_legend`
  (`src/ui/replay/viewer/`); plus the check that `replay::measurement`'s
  mirrored copy of the `wasm32`-only `BROWSER_REPLAY_LIMIT` still matches
  `platform`'s. Source text is the only view a native `cargo test` has of a
  `wasm32`-only constant, and the only view a `--no-default-features` run has
  of a `client` module. The scan is raw file text, so a doc comment or a test
  that repeats a retired phrase fails exactly like production code would; a
  failure naming `sources_in` is about wording, not behavior, and the phrase
  list is in the named test.
- **The module dependency graph**, described in `docs/ARCHITECTURE.md`'s
  [Module ownership and dependency direction](ARCHITECTURE.md#module-ownership-and-dependency-direction)
  section.
- **The printable-ASCII rule for in-game text**, held by
  `production_string_literals_are_printable_ascii` in `architecture.rs`
  itself. `CONTRIBUTING.md`'s "UI and accessibility" section requires it
  because Bevy's bundled fallback font draws anything else as a
  missing-glyph box, and two player-facing strings had drifted past the rule
  before anything checked it. Unlike the negative assertions, this scan is
  not raw text: it lexes every production file, keeps only string literals,
  drops every item behind a `cfg` that only holds under `test`, and decodes
  escapes, so a doc comment may name an accented letter, a
  `#[cfg(test)]` fixture may type one, and `"\u{2014}"` fails exactly as a
  bare em dash does. What it does not read - `char` literals, and any text
  built at runtime - is listed in the test's own doc comment, which is the
  place to check before relying on it. The test runs in both build
  configurations, since source text needs neither feature.

- **The rule that colours are made in one file**, held by
  `colours_are_constructed_only_in_presentation` in `architecture.rs`. It
  lexes every production file the way the ASCII guard does, and reports any
  associated item of `Color` other than `Color::NONE`, a named colour on a
  wrapper such as `TextColor::WHITE`, any of the other colour types, and any
  call that derives a colour or changes one in place (`with_alpha`,
  `set_alpha`, `lighter` and the like) outside `src/presentation.rs`. What
  it cannot see is a colour nobody wrote - a default - and its doc comment
  lists those. A test may still compare
  against a `Color::srgb(...)` literal, and a doc comment may name one. It
  requires a floor of constructions *inside* `presentation.rs`, so a scan
  that stopped recognising a constructor fails rather than finding the rest
  of the crate clean.
- **The completeness of the contrast table**, held by
  `every_colour_constant_is_in_the_table` in
  [`src/contrast.rs`](../src/contrast.rs). It reads `presentation.rs` as
  text, takes every line that begins `pub(crate) const NAME: Color =` as a
  colour, and requires each name to appear in at least one row of the table.
  `every_colour_made_by_a_function_is_in_the_table` beside it does the same
  for the functions in that file that build a colour, which it finds by
  running the first guard's scan over each function's body.
  The two guards are a pair: the first is what makes "every colour constant"
  mean every colour, and the second is what makes a new one say where it is
  drawn. The rest of `contrast.rs` is ordinary arithmetic over the constants
  and reads no source; `CONTRIBUTING.md`'s
  [UI and accessibility](../CONTRIBUTING.md#ui-and-accessibility) section
  states the floor it checks and what an exemption has to say.
- **The text-size floor**, held by `no_text_is_set_below_the_floor` in
  `architecture.rs`. It lexes every production file the way the colour
  guard does and reads the argument of each `FontSize::Px`: a number, or a
  name it looks up among the crate's `const NAME: f32 = <number>;`
  declarations. Anything else - an expression, a parameter, another unit, a
  `TextFont` builder, an `.into()` - fails as unreadable rather than
  passing, which is the point: the audit that first found 11px text was a
  grep for numbers, and missed a 13px size set through a constant. A
  `font_size` given a plain name passes, since that is a `FontSize` made and
  read elsewhere; the test's doc comment lists what else it cannot see. It
  requires a floor of sizes read, like the colour guard's floor of
  constructions.

All six read from disk with `CARGO_MANIFEST_DIR`, not `include_str!`. The
difference is the whole point and is worth stating once: `include_str!` needs
a list of files written by hand, and a negative assertion **cannot fail
because of a file missing from that list** - it is silently uncovered. That
is not hypothetical here; the `replay` list lost `catalog.rs` when the
catalog was extracted and nothing noticed. A directory walk covers a new
sibling the moment it exists.

The hazard a walk introduces instead is scanning too little and passing
vacuously, which is harder to spot than a stale list. So `sources_in` takes
the number of files it must find, panics below it, and panics if the
directory does not exist; two `#[should_panic]` tests hold both. The count is
a floor, not a manifest - a new file raises coverage silently, while removing
one is a visible edit somebody makes on purpose. The crate-wide walks carry
the same floor on their file count, and the ASCII guard one more on the
number of literals it read, so a lexer that classified everything as a
comment would fail rather than find nothing to object to.

Writing a new one of these: call `sources_in("<dir>", <count>)` or
`source_file("<path>")` from `crate::architecture` rather than adding an
`include_str!`, and give the count the directory's current file count.
Assemble the retired phrase at runtime (`["Nei", "ther"].concat()`), never as
one literal, the way every existing negative assertion does: the walk reads
the test's own file, so a literal would fail the assertion it belongs to.

## Pure tests versus ECS tests

Prefer a pure test when the behavior can be expressed as values:

- board before + two `LockedMove` values -> `RoundResolution`;
- two ordered opening windows -> capture-free Round 0 reveal;
- JSON text -> migrated `ControlsConfig`;
- Replay + timestamp -> `ReplayFrame`;
- a button's previous and current hover state -> optional hover `UiCue`.

Use a Bevy `App` when behavior depends on:

- system order or run conditions;
- resources changing across frames;
- keyboard/gamepad injection;
- state entry/exit and entity cleanup; or
- deferred commands.

`src/test_support.rs` advances test time deterministically and, through
`capture_log_events`, asserts on what a system actually logged.
`src/gameplay/test_support.rs` builds reusable Match input pipelines.
`src/ui/replay/test_support.rs` pins a fixed id and label onto the shared
`replay::test_support::sample_replay` so the replay screens' tests can assert
on rendered metadata strings without depending on the clock; it builds on that
recorder-built fixture rather than a struct literal so it stays a `Replay` that
could really exist in storage.

`src/ui/focus.rs` carries `real_layout_app`, which runs the production UI
headlessly against a window of the size it is handed: `bevy_ui`'s own
plugin - layout, its text systems and the font the game draws with,
stacking and visibility - with `bevy_picking`, its UI backend and the
crate's `ui::button` plugin, but no renderer. A `Text` node is sized by its
glyphs and its glyphs are placed, so it answers where text is *drawn* as
well as where its node is, which can differ - see the next section. Every
screen's tests build on it.

The size is the window's and the app's `WindowSize` alike, so a screen that
lays itself out from the window's size is laid out for it. A test of a
screen as it was first laid out passes the opening size,
`WindowSize::OPENING.0`, 1040x900. A screen laid out for any size is
tested at several:
`app::visuals::NAMED_WINDOW_SIZES` names the ones that matter, from the
1040x600 minimum and a 1280x720 laptop's maximised window up to 2560x1440,
and the Match screen, Home and the Replay Viewer run their real-layout
tests at all of them - every button reached by the pointer, every text
inside the window, and on the Match and the Viewer nothing drawn meeting
anything else. Their layouts are pure functions as well
(`ui::match_screen::geometry`, `ui::home::geometry`,
`ui::replay::viewer::geometry`) - of the size, and the Viewer's of the
room its Replay's label needs too - so their own tests run every size in
`app::visuals::layout_sweep` - every 20 px from the minimum to 2560x1440,
the named sizes, a few fractional ones and a few larger windows - and
check the same things from the layout's own boxes. The two halves
check different things: the sweep reaches every size, at the cost of
trusting the layout's own idea of where each box is, and the real layout
checks that idea against `bevy_ui` at the sizes it runs. The room the
Viewer's label needs is the one input that comes from a text: its lines
are counted as `bevy_ui` breaks them (`ui::replay::viewer::wrap`), and
`the_label_check_counts_the_lines_bevy_ui_sets` lays nearly twenty thousand
labels out with `bevy_ui` at seven widths - the label's in narrow, middling
and wide tall windows, wide's column, and three that are an exact number
of characters - and holds the line each character is drawn on, read from
its glyph, to the line the count puts it on, character by character. The
labels put every punctuation mark at every place around a full line's end,
and every two marks side by side there, so a change to any one
line-breaking rule, or to the class of any one character, moves some
character to another line there.

It is also how a test presses a button. `ui::button` carries the helpers -
`click`, `hover`, `mouse_button`, `queue_mouse_button` for a key in the same
frame, `touch` - and they write the same window events winit writes, so a
click goes through hit-testing, bubbling, the press and the release, and
nothing in a test writes a button's state by hand. A test of what a
handler does with an intent can trigger the activation itself with
`trigger_activation`, which is the `Activate` every input produces; a test
of how a button comes to be activated should press it.
`reachable_buttons` and `covered_at_their_centres` sweep a screen for
anything drawn over a button: picking stops at the first node under the
pointer that has no `Pickable` saying otherwise, whatever that node is, and
each screen has a test that holds every control it shows to it, with and
without its dialogs open. Before 2026-10-27 this was two apps - layout
alone, and layout with text - built from `bevy_ui`'s systems one by one;
moving it onto `bevy_ui`'s plugin changed no layout result any test
asserts.

Beside it, `measure_drawn_text` reads where one `Text` was
drawn on such an app: its glyphs' inked box, and the nearest filled node
under it. `measure_drawn_lines` reads the glyphs alone, line by line, for
text drawn straight onto the window with no filled node beneath it; it is
also the way to ask whether a line wrapped. The Match screen's
`the_longest_end_reason_line_fits_on_one_line_clear_of_both_stacks` uses
it with the font's real advance, 0.6 em or 9.6 px a character at 16 px,
which is what makes a line's width predictable from its character count
before a run. `ui::text_metrics` is that prediction, for a layout that has
to know before `bevy_ui` runs whether a line fits, and its test holds it to
what `bevy_ui` lays out. For a test that holds a whole screen inside the
window or clear of itself, `drawn_texts` reads every text drawn at once,
by its glyphs - the Match's overlap test uses it - and `laid_out_texts`
every text by its node, which is how the Match's and Home's tests hold
every text inside the window and the Match's readouts beside the heading
centred on its line.

The Settings Game tab's `the_game_tab_fits_its_panel_at_its_most_crowded`
uses the same app on a panel that clips, and two things it learned are worth
knowing before measuring any other. A node with a `max_height` and
`Overflow::clip()` takes its content's height up to that maximum and hides
the rest, but its children do not visibly spill when the content is taller:
in a flex column too short for them they *shrink*, so every row's box still
ends inside the panel while its text is cut off. Compare the container's
laid-out height with its `max_height` instead - reaching it is the overflow.
And a row whose children are wider together than the row squeezes all of
them, controls included, by fractions of a pixel each; a squeezed `Text`
wraps. So the test checks each control is laid out at its declared width
and each caption on one line, which is how it found the Match Time row's
caption drawn as `Min` above `60:00` at the old 12 px (2026-10-07).

### A Text node does not centre its own text

In `bevy_ui`, `justify_content` and `align_items` place a node's children,
and a `Text` node's glyphs are not its children. They are laid out from the
top-left corner of the node's content box: `TextLayout::justify` can move
them across inside it, and nothing in Bevy moves them down. That was
measured on 0.19.1 and again after the upgrade to 0.20.0 (2026-10-27): a
300 by 200 px `Text` node asking for centre, end and start alignment in
turn drew its 64 px digit with the ink's centre about 60 px above the
node's, identically on both versions. So a node
that carries `Text` *and* a fixed size, a fill, a border and centring
alignment - a card - draws its text in its own top-left corner, as ordinary
text would sit, while every node rect on the screen reads correctly. The fix
is structural: make the box a plain node and the `Text` its child, sized by
its glyphs, which the box's alignment then centres.

That is how the Replay Viewer's countdown card had always drawn its `3`, until
2026-10-05: 56 px left of its centre and 15 px high, with the card itself
exactly where its spawner put it. Both
countdown cards now come from one spawner, `ui::countdown_card`, whose module
docs carry the detail. Two things follow for anyone checking an overlay:

- **Node rects cannot see it.** The card's `ComputedNode` was right, and the
  whole-screen dump under [Manual smoke checks](#manual-smoke-checks), which
  compares boxes, would have shown nothing wrong. Read the glyphs instead:
  `TextLayoutInfo::glyphs` gives each glyph's centre in physical pixels from
  the content box's top-left, and `atlas_info.rect` its inked size. That is
  how `measure_drawn_text` in `ui::focus` reads text, for the
  `the_countdown_digit_is_drawn_centred_on_a_card_over_the_board` tests on
  the Match screen and in the Viewer, and for
  `the_empty_list_message_is_drawn_where_the_first_card_stands` in the
  Replay Library.
- **The same fault had one other instance, and it is fixed.** The Replay
  Library's empty-list message was a `Text` node 688 px tall asking for
  `align_items: Center`, and its lines were drawn against the top edge of
  the list: 317 px above the node's centre for the two-line message and
  330 px for the one-line one, measured in the running game and through
  the headless real-layout app alike. Since
  2026-10-06 it is a `Text` child of a plain node one card tall at the top of
  the list, so it stands where the first card would - a placement chosen
  over the middle of the list, for reasons `spawn_empty_list_message` in
  `ui::replay::library::layout` gives. A census of all 78 `Text` spawn
  sites in `src/` that day, following each one's `Node` through helpers,
  variables and parameters, found no other node asking to be centred down
  its box. Seven carry `justify_content: Center` - the Match screen's
  heading and capture counts, the Library's rename field, the Viewer's
  timer, round and result lines, and the Settings mapping error - which in a
  row asks only for the horizontal centring their
  `TextLayout::justify(Justify::Center)` already gives, so it does nothing
  and shows nothing. A scan that reads only the literal in each spawn
  misses the first two, whose `Node` arrives as a parameter and a variable.

Two more came from laying the Match screen out for any window size, and
follow for anyone placing a text inside a box of its own:

- **A text whose line count varies centres only as a child.** The Match
  screen's round panel, beside the heading in a wide window, is one line,
  or two while Round Time runs. Placed by a `top` worked out for two lines,
  its one-line form stood 10 px above the heading's line (2026-11-27). It
  is now a plain node as tall as the title row, holding the `Text`, which
  its `align_items: Center` centres whatever the line count, as the Match
  timer's panel always did;
  `the_readouts_beside_the_heading_stand_centred_on_its_line` holds it.
- **A justification is not free either.** Right-justifying that same text,
  one line in a node exactly its own width, moved its glyphs by a fraction
  of a pixel: ImageMagick's `compare -metric AE` counted 75 differing pixels
  in the 1040x900 capture at twice its size, all in the glyphs of
  `Round 0` (2026-11-27). Wrapping it in the plain node changed none. So a
  change meant to leave a screen pixel-identical keeps each text's
  justification as it was, and the round panel is right-justified only
  beside the heading.

## Temporary persistence roots

Persistence tests construct `ReplayStore` or save settings beneath unique
temporary directories. They do not call user-path loaders and must never touch
real `controls.json` or Replay libraries.

Clean up temporary directories at the end of a successful test. When adding a
failure-path test, choose a unique root so parallel tests cannot collide.

Test all supported historical versions with literal JSON or programmatically
constructed old-version values. The literal files live under
`tests/fixtures/replays/` (one per Replay schema version plus richer shapes;
its README explains how they are pinned and when to regenerate them), and a
new schema version adds a new file there without changing any older one.
Replay v1-v3 fixtures must continue to begin in ordinary Round 1, while v4
tests cover Round 0 events, bounded unique opening arrays, countdown audio,
reveal reconstruction, and the separate `opening_completed` metadata. A
current round-trip test is not a substitute for migration coverage.

Replay v5 tests also cover immutable Match settings, bounds and integer-second
durations, ordered simultaneous end reasons, timeout cause/deadline/submission
validation, per-round capture deltas, interpolated round clocks,
capture-aware progressive scores, and silent seeking. A reveal whose sweep
removes a stone is covered by `v6-sweeping-reveal.json` and by driven-Match
tests in `gameplay::countdown`: the recorded reveal frame carries the cleaned
board and the sweep's credits, and validation re-derives both from the
persisted selections.
Viewer tests assert the Back/Match Settings navigation contract, modal
exclusion and cleanup, old-Replay Off/Legacy defaults, and fit at every
window size from the 1040x600 minimum up, as above.

Run the same storage contract against an in-memory/mock backend in native tests.
It should cover successful reads and writes, missing values, invalid serialized
data, deletion, enumeration, and write failures without needing a browser.
Native in-module tests (`settings::store` and `platform::native`) additionally
cover atomic files and adjacent-executable asset resolution with the
current-working-directory development fallback.

Browser persistence is also a compile-time and behavioral contract: Settings
persist when same-origin storage is available, and completed Replays retain
exactly the newest three. Test deterministic eviction and quota/blocked-storage
failure through the mock. `scripts/build-web.sh` compiles the real adapter for
`wasm32-unknown-unknown`, then verifies the Trunk output rather than attempting
to emulate browser APIs in a native test.

Two measurement tools exist alongside the ordinary test suite, both
deterministic and both primarily reporting rather than gating: `cargo test
replay_size_measurements -- --nocapture` (`src/replay/measurement.rs`)
reports serialized Replay size across representative scenarios as one
number, because a native file and a browser record hold the same compact
encoding, and checks each scenario against both per-replay caps; it measures
the two write paths separately and asserts they agree, so a writer that
drifts from the browser encoding is caught there rather than in a size
conclusion drawn against the wrong bytes, and `cargo
test --release replay_save_timing_measurements -- --nocapture`
(`src/gameplay/save_timing.rs`) reports native save timing on real, fully
valid Replays - run the latter with `--release`, since debug-build timings
do not reflect what a player's machine experiences. Neither asserts a
specific number; both exist so a future change that meaningfully affects
size or save time gets noticed and re-documented rather than silently
drifting from the numbers recorded in `docs/ARCHITECTURE.md`. The size tool
makes one exception and fails outright when a scenario exceeds the native
per-replay cap, because "every modelled shape fits" is a conclusion that
document now states; it still only reports the browser cap, which several
scenarios exceed by documented, accepted design.

## Diagnostics and failure paths

`src/diagnostics.rs`'s own tests are pure and Bevy-resource-only (no window):
they cover deduplication of an identical consecutive push, that two distinct
diagnostics both survive, that a repeated message separated by a different one
is not treated as a duplicate, and that the active log stays bounded.

Nothing reads `Diagnostics` back in production, so the player-safe messages
would otherwise never be exercised - which is exactly how a call site once
came to push its *technical* message onto the screen and its player-safe one
into the log nobody reads. Every site that pushes a `Diagnostic` - the five
`report_recoverable` call sites in `gameplay`, `ui::settings`, and
`ui::replay::library`, plus the one direct push in `app::startup_diagnostics`,
which runs before `LogPlugin` exists and so cannot log through
`report_recoverable` - therefore has a test that drives its real failure and
asserts the exact `Diagnostic` message, plus that the message contains no path
separator and no `os error`. The shared guard is
`diagnostics::assert_carries_no_internal_detail`, and its boolean form
`carries_internal_detail` (both `#[cfg(test)]` in `src/diagnostics.rs`): three
substring checks for a path separator or `os error`, deliberately crude
because every technical string in this crate is a formatted
`SettingsStorageError` or `ReplayError` wrapping a path or a `std::io::Error`.
Assert that in both directions: also check that the technical string the same
failure produced *does* trip that guard, or a mock whose own message happens
to be tidy will let a leaking call site pass. `PathLeakingReplayStorage` and
`LEAKED_REPLAY_STORAGE_ERROR` (`src/replay_library/test_support.rs`) exist for
that, as do two `PathLeakingSettingsStorage` mocks: a save-failing one in
`src/ui/settings/mod.rs`'s tests and a load-failing one in `src/app/mod.rs`'s
tests, each with its own path-carrying `LEAKED_SETTINGS_ERROR`.

Asserting that a log line is *emitted*, rather than that the code producing
it ran, needs `capture_log_events` (`src/test_support.rs`), which
`app::startup_log`'s tests use to prove the deferred startup warnings really
come out of a `warn!`, and which `app`'s
`a_failed_settings_loads_detail_is_logged_by_the_startup_system` uses to show
the real settings-load failure's path and OS error reach that log line rather
than the player. Two constraints shape it, and both fail silently -
as an empty capture that reads exactly like a system that logged nothing -
if ignored:

- The subscriber must be **scoped** (`tracing::subscriber::with_default`),
  never global. `tracing`'s global default can be set once per process and
  `LogPlugin` is what sets it, so a test calling `set_global_default` would
  capture nothing as soon as any other test in the same binary got there
  first.
- A scoped subscriber is **thread-local**, and Bevy hands schedule systems to
  the task pool, so the event is emitted on a thread that cannot see it. The
  helper forces the `Startup` and `Update` schedules onto the calling thread
  with `SingleThreadedExecutor` first.

Everywhere a subsystem's typed error reaches a diagnostics boundary
(`SettingsStorageError`, `ReplayError`), prefer a controllable mock over real
filesystem permissions, browser quota, or wall-clock timing:
`MockSettingsStorage` (`src/settings/store.rs`, `fail_load`/`fail_save`
booleans), `MockReplayStorage` (`src/replay_library/test_support.rs`, a
`fail_apply` atomic covering the `ReplayStorage` trait's single mutation entry
point), and `FlakySettingsStorage` (`src/ui/settings/mod.rs`, a `fail` atomic a
test can flip mid-session) let a test force exactly the failure it wants
deterministically; `FailingSettingsStorage` and the `PathLeaking*` mocks carry
no flag at all and fail unconditionally. Cover both directions - a storage
backend that cannot even be read (falls back to defaults with a warning, not an
error the caller must handle) as well as one that rejects a write (the
in-session change stays active; only persistence failed) - since a real bug can
hide in either half alone.

## Injected controllers and semantic input

Controller tests spawn Bevy `Gamepad` components and explicitly populate
`ControllerAssignments`. This exercises the same profile lookup and semantic
fusion as the game without requiring hardware.

When changing input:

1. test the default mapping;
2. test a remapped keyboard key or controller input;
3. test `pressed` and `just_pressed` behavior;
4. test disconnect/release behavior when relevant; and
5. prove gameplay consumes `GameAction`, not a physical key/button.

Analog tests should cross both hysteresis thresholds. Values near the press
threshold must not chatter.

## Audio tests

Most audio tests inspect `AudioCueQueue` rather than using speakers. Gameplay
tests assert semantic cues and Replay tests assert when cues cross the playback
clock. This makes sound behavior deterministic and headless.

The WAV integrity test reads committed assets and verifies channel count, sample
width, sample rate, length, clipping headroom, and click-safe endpoints.
`tools/generate_marimba.py --check` independently regenerates all marimba notes
and the Round 0 duplicate-selection clack in a temporary directory and compares
bytes without modifying the repository.

## Adding a regression test

Name the observable rule, not the implementation. Reproduce the boundary value
that matters: for example, timestamps on both sides of 0.1 seconds, a held input
through `Go!`, overlapping Replay selections, or a corrupt file beside a valid
one.

For a bug fix:

1. add a failing test that demonstrates the old behavior;
2. fix the owning module;
3. run the focused module;
4. run the complete release check set; and
5. update rules or architecture docs when the contract changed.

Do not weaken existing assertions, sleep on wall-clock time, depend on a real
controller, play through a real audio device, or use the contributor's
application-data directory.

If the regression is that two structures drifted apart - a table and the code
it describes, a constant and its mirror - prefer a test that derives one side
rather than one that restates it. See
[Tests that read the crate's own source](#tests-that-read-the-crates-own-source)
above, and `docs/ARCHITECTURE.md`'s
[Mirrored structures and the two-layer guard](ARCHITECTURE.md#mirrored-structures-and-the-two-layer-guard)
for when a test is the only half available.

## Manual smoke checks

Automated tests do not verify window creation, and they check visual fit only
where a test lays a screen out and measures it. For UI, startup, or
asset changes:

1. run `cargo run`;
2. confirm the 1040x900 Home screen opens;
3. navigate Home -> Settings -> Home -> Match -> Home -> Replays;
4. confirm each screen appears once and no previous entities remain;
5. confirm Round 0 dots/cursor privacy and countdown/clack/marimba sounds behave
   without terminal warnings;
6. open the Help modal (Help, during a match) and page through every page, confirming each
   page's text ends clear of the pagination row - the modal never scrolls, and
   no automated test measures whether the copy still fits; and
7. close the window normally.

Step 6 is the only check on help-page length, so it is worth knowing what the
margins are. Measured from screenshots of the original nine pages at
1040x900, and by the probe below for the two terms pages added on
2026-10-05, as the gap between the last line of body text and the top of the
pagination row, against a body line pitch of about 21.5 px:

| page | room left |
| --- | --- |
| Keyboard Controls | 29 px |
| Terms: Points, Groups & Liberties | 50 px |
| Terms: Gray, Suicide, Ko & Territory | 50 px |
| Round 0: Hidden Opening | 75 px |
| Round 1+: Hidden Moves & Timing | 28 px |
| Captures, Repetition & Gray | 50 px |
| Ending, Territory & Restart | 180 px |
| Match Rules | 140 px |
| the three controller pages | 331 / 331 / 29 px |

The Keyboard Controls and Shared Controller Actions rows were re-measured
when the menu-keys and going-back blocks were added, and again when Start
became the pad's back on every screen, not from a screenshot but from the
ECS of the running game: a temporary system reading the body's
`ComputedNode` + `UiGlobalTransform` (physical px) and `TextLayoutInfo`
(logical px in Bevy 0.19; 0.20 gives its size in physical px, and the
probe below reads it that way) against the pagination row's node. That
glyph-box metric sits about 5 px under a screenshot of the same build - the
layout box carries trailing line padding - so the values above add that
calibration back in. On the second measurement it read 66.5 px for
Keyboard Controls, unchanged, and 23.5 px for Shared Controller Actions,
whose menu-buttons block now
wraps to two more lines; the same probe reproduced the other seven rows'
screenshot numbers to within 3 px, which is how the calibration is known
to hold. A third measurement, taken on 2026-09-25 when the Help
modal joined the focus ring and both menu blocks were reworded to say so,
read 23.5 px for both pages. Keyboard Controls had already given up most of
its room to the second measurement's rewording; this one cost it the rest,
and the Shared Controller Actions block was tightened back to its previous
line count after a first wording ran one line over, which the probe showed
as the body pushing the pagination row and Close 16
px down the modal - exactly the failure this measurement exists to catch. A
fourth, on 2026-09-26 when F1 became the Help key, read 23.5 px for
both pages again: each block swapped one sentence for another of exactly the
same length, so neither page gained or lost a line, and the identical text
heights said so before the room did. A fifth, also on 2026-09-26, measured
only the Ending, Territory & Restart page, which gained the sentence saying
Restart never interrupts the end gesture: 239.5 px before the sentence and
218 px after by the glyph-box metric, one 21.5 px line apart, which is the
244 px to 223 px move in the table once the calibration is added back. A
rewording of that sentence measured 218 px again, so the table stands. The
two tight pages were not touched, so they were not re-measured. A sixth,
on 2026-10-03, measured only the Captures, Repetition & Gray page, which
gained a two-sentence paragraph on the capture marks: 45 px by the
glyph-box metric, the 50 px in the table once the calibration is added
back, so that page has now spent most of its room too. A seventh, on
2026-10-03, measured only the Round 0 page, whose first two paragraphs were
reworded to teach the dot row's line and cross: 66 px by the glyph-box
metric both before and after, because each paragraph was held to its old
four lines. That is 71 px with the calibration, against the 75 px in the
table from the original screenshot, so the table stands; the 4 px is the
calibration's own spread, and the identical text heights (476 px) are what
show nothing moved. An eighth, on 2026-10-04, re-measured the Captures page
after its paragraph was reworded to say the outline is what reduced motion
shows: 45 px again, so the table stands there too. A ninth, on 2026-10-05,
measured the two terms pages the help gained between Keyboard Controls and
Round 0: 45 px each by the glyph-box metric, the 50 px in the table once the
calibration is added back, at 23 wrapped lines apiece - the count
`textwrap` at 69 had predicted before the run. The same run re-read Round 0
at 66.5 px and Shared Controller Actions at 23.5 px, both unchanged, so the
shift in page numbers moved nothing. It also caught what no body metric
can: the page indicator's node was 120 px wide, which fitted `Page 9 of 9`
and wrapped `Page 2 of 11` onto two lines the moment the count reached two
digits. Only the capture showed it; the node is 150 px now, and the probe
below prints the title's glyph box beside the body's for the same reason.
The two terms titles are the longest in the help, 33 and 36 characters
against 31 before them, and measured one line high at 594 and 648 logical
px in the 752 px title row - so a title of about 41 characters is where
that row would wrap and push the body down a line. A tenth, on 2026-10-05,
measured only the Ending, Territory & Restart page, which gained the
sentence saying the bar drains at half speed when a Select slips rather
than emptying: 174.5 px by the glyph-box metric, two 21.75 px lines below
the 218 px of the fifth measurement, which is the 223 px to 180 px move in
the table once the calibration is added back. `textwrap` at 69 had
predicted seven lines for the first paragraph against five, and the probe
confirmed it; with the longest plausible key labels the paragraph wraps to
eight, which the room still holds comfortably. The title stayed one line. An
eleventh, on 2026-10-27, measured all eleven pages before and after the
upgrade from Bevy 0.19.1 to 0.20.0, each version in its own build: every
text height, room and title box came out the same on both, to the half
pixel, so the table stands. That run is also how the unit change above was
found - the probe as it stood, multiplying the layout's size by the scale
factor, reported -495 px of room on Keyboard Controls under 0.20.

The help body wraps at 69 characters of its monospace font, which makes a
rewording's line count predictable before any run: wrap the paragraph at
69 with Python's `textwrap` and count. The 2026-10-03 rewording used that
to find wordings that kept both paragraphs at four lines, and the probe
then confirmed it. It is a guide, not a substitute - the probe is still
what says a page fits.

The probe itself used to be rewritten from this description each time it
was needed, so here it is. Save it as `src/ui/layout_probe.rs`, add
`mod layout_probe;` beside the other modules in `src/ui/mod.rs`, register it
with `app.add_systems(Last, layout_probe::layout_probe)` at the top of
`UiPlugin::build`, build, and run the binary with `GT_PROBE_PAGE` set to the
page you changed (zero-based); it does nothing without that variable, so it
can stay registered across several runs. It opens the modal by setting
`HudState` directly, measures on three consecutive frames so a stale layout
would show as a changing number, prints the title's glyph box beside the
body's so a title that wraps is caught too, and exits on its own. With
`GT_PROBE_OUT=<png>` set as well, the same run saves the whole 1040x900
window with the modal open, at twice the logical size, the way the capture
probe under "Capturing a screen to a file" below does - which is how a
capture to look at and the measurement come from one build and one command
each. Remove all of it before committing: nothing in the tree
should print, and the source-reading tests in `src/architecture.rs` would
read it as production code.

```sh
cargo build
GT_PROBE_PAGE=1 ./target/debug/go-together
GT_PROBE_PAGE=1 GT_PROBE_OUT=/tmp/terms-1.png ./target/debug/go-together
```

```rust
use bevy::app::AppExit;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::text::TextLayoutInfo;
use bevy::ui::{ComputedNode, UiGlobalTransform};

use super::hud::{HudState, UiRole};
use crate::app::visuals::{WINDOW_HEIGHT, WINDOW_WIDTH};

/// Rendered at twice the logical size, as on a high-density display.
const SCALE: f32 = 2.0;

#[allow(clippy::too_many_arguments)]
pub(super) fn layout_probe(
    mut commands: Commands,
    mut frames: Local<u32>,
    mut target: Local<Option<Handle<Image>>>,
    mut images: ResMut<Assets<Image>>,
    mut hud: ResMut<HudState>,
    texts: Query<(
        &ComputedNode,
        &UiGlobalTransform,
        &TextLayoutInfo,
        &UiRole,
        &ChildOf,
    )>,
    nodes: Query<(&ComputedNode, &UiGlobalTransform, &ChildOf)>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(page) = std::env::var("GT_PROBE_PAGE") else {
        return;
    };
    let page: usize = page
        .parse()
        .expect("GT_PROBE_PAGE is a zero-based page index");
    let out = std::env::var("GT_PROBE_OUT").ok();
    *frames += 1;
    let frame = *frames;

    if frame == 20 && out.is_some() {
        let mut image = Image::new_target_texture(
            (WINDOW_WIDTH as f32 * SCALE) as u32,
            (WINDOW_HEIGHT as f32 * SCALE) as u32,
            TextureFormat::Rgba8UnormSrgb,
            None,
        );
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let handle = images.add(image);
        commands.spawn((
            Camera2d,
            Camera {
                order: -1,
                ..default()
            },
            RenderTarget::Image(handle.clone().into()),
            Projection::Orthographic(OrthographicProjection {
                scale: 1.0 / SCALE,
                ..OrthographicProjection::default_2d()
            }),
            Transform::from_xyz(0.0, 0.0, 0.0),
            // bevy_ui draws to the default UI camera and lays itself out in
            // that target's own pixels, so it needs both of these to appear
            // at all and to appear at the size of the board it surrounds.
            IsDefaultUiCamera,
        ));
        commands.insert_resource(UiScale(SCALE));
        *target = Some(handle);
    }
    if frame == 40 {
        hud.help_open = true;
        hud.help_page = page;
    }
    if (60..=62).contains(&frame) {
        let mut body = None;
        let mut title = None;
        let mut indicator = None;
        for (node, transform, layout, role, child_of) in &texts {
            match role {
                UiRole::HelpBody => body = Some((node, transform, layout)),
                UiRole::HelpTitle => title = Some((node, transform, layout)),
                UiRole::HelpPage => indicator = Some(child_of.parent()),
                _ => {}
            }
        }
        // The pagination row is the page indicator's grandparent.
        let (_, _, indicator_child_of) = nodes.get(indicator.unwrap()).unwrap();
        let (row_node, row_transform, _) = nodes.get(indicator_child_of.parent()).unwrap();
        let (body_node, body_transform, body_layout) = body.unwrap();
        let (title_node, _, title_layout) = title.unwrap();
        let scale = 1.0 / body_node.inverse_scale_factor;
        let body_top = body_transform.translation.y - body_node.size.y / 2.0;
        // Bevy 0.20 gives a text layout's size in physical px, as the
        // nodes are; 0.19 gave it in logical px.
        let text_height = body_layout.size.y;
        let pagination_top = row_transform.translation.y - row_node.size.y / 2.0;
        let room = pagination_top - (body_top + text_height);
        println!(
            "PROBE page {page} frame {frame}: text height {text_height:.1} (node {:.1}) -> room {room:.1} physical px = {:.1} logical px (scale {scale}); title glyph box {:.1}x{:.1} in node {:.1}x{:.1}",
            body_node.size.y,
            room / scale,
            title_layout.size.x,
            title_layout.size.y,
            title_node.size.x,
            title_node.size.y,
        );
    }
    if frame == 80
        && let (Some(out), Some(handle)) = (out.as_ref(), target.clone())
    {
        commands
            .spawn(Screenshot::image(handle))
            .observe(save_to_disk(out.clone()));
    }
    if frame == 140 {
        exit.write(AppExit::Success);
    }
}
```

Hidden Moves, Keyboard Controls and Shared Controller Actions are the pages
to re-measure first; each is within a line and a half of the row. The pages do not share vertical
space - `help_page` returns one page's body and the modal shows one at a time
- so lengthening one can never overflow another, and one probe run on the
page you changed settles it.

**The Replay Viewer's Key is not a help page**, and neither the margins
above nor this probe apply to it. It has no pagination row and no fixed
height: its panel is as tall as its rows and centred in the window, so a
longer row cannot run into anything inside the panel; it moves the panel's
top and bottom edges toward the window's instead. What has to hold is that
the panel fits the window, and since 2026-11-28 a test holds that:
`the_key_stands_inside_the_window_in_either_form` in `ui::replay::viewer`
lays the Key out at every named window size and at either side of the
height where it turns to two columns, holds it 12 px inside the window,
and holds its height in each form to the figure measured for it, read from
the `ComputedNode` of its panel at a scale factor of 1: 698 px in one
column, which is what the layout chooses the Key's form by, and 494 in
two. At twice that scale it measured 695 px in one column, on that day's
captures - text rounds up to whole physical pixels, which adds less at a
finer scale - and below a scale of 1, as in a zoomed-out browser, it was
not measured. A row that grows changes those figures and fails the test
until they are measured again. In one column a Key sentence wraps at 65
characters - 624 px of 16 px monospace - so the same `textwrap` count
predicts its lines, and each line costs about 19.5 px; in two columns, at
44. The score badges' sentence points to the result line, which tall and
wide put in different places, so it has a wording for each arrangement;
the two wrap at the same places at either width, and the test holds which
the Key shows in each arrangement: in tall, the sentence it has always
shown, byte for byte.

### Capturing a screen to a file

How a board or a screen *looks* is a question no test answers, and it is the
question a colour or layout change raises. A second camera that renders to an
image, saved with Bevy's own screenshot observer, answers it from a script: it
needs no window capture and no permission, and it sees `bevy_ui` as well as the
world-space board. The contrast-floor work used it for every before-and-after
image, and to measure one pixel the contrast guard now pins (see
`the_arithmetic_matches_known_values` in `src/contrast.rs`).

Four things about it are not obvious, and each cost a failed capture to learn
on 2026-09-30 with Bevy 0.19.1. All four were checked again on 2026-10-27
with Bevy 0.20.0, where each still holds - the scaled target still saves
solid black, and the blended pixel below is the same - and the probe below
compiles and runs unchanged. A fifth came with the layouts that adapt to
the window, on 2026-11-27:

- **Scale with the projection, not the target.** Rendering at twice the
  logical size is `OrthographicProjection { scale: 0.5, .. }` on an image twice
  as large. `ImageRenderTarget { scale_factor: 2.0, .. }` looks like the same
  thing and renders nothing at all - the saved image is solid black.
- **`bevy_ui` draws only to the default UI camera.** Without
  `IsDefaultUiCamera` on the capture camera the image holds the world-space
  sprites and meshes alone, which is enough for a board and useless for a
  menu.
- **`bevy_ui` lays itself out in the target's pixels.** At twice the size,
  that is an interface half as large as the board it surrounds, in a corner of
  the image. `UiScale(2.0)` puts it back.
- **Translucent colours are blended in linear light**, for `bevy_ui` nodes and
  world sprites alike. A 96%-opaque dark panel over the Home screen's orange
  diamond is sRGB (56, 34, 23), not the (23, 22, 22) that blending the encoded
  values would give. Anything that predicts a composite has to do the same.
- **The main camera frames the world.** A screen that lays itself out for
  the window's size places and scales its world-space scene through the
  main camera's projection and translation (`WorldFraming` in
  `app::visuals`), not by moving the scene. In the opening window that is a
  scale of 1 over the origin, which is what the probe below assumes. A
  capture of a screen laid out for any other size - by writing the probe's
  size into `WindowSize` before the screens' layout passes run - needs its
  own camera to take the main camera's projection scale, divided by the
  capture's `SCALE`, and its translation, every frame, or the board is
  drawn at its own size about the image's centre, wherever the layout
  around it has made room for it.

The probe below is the one that work used. Save it as
`src/ui/replay/viewer/board_probe.rs` - it lives there because the Viewer's
state is private to that module - add `mod board_probe;` beside the other
modules in `src/ui/replay/viewer/mod.rs`, and register it with
`app.add_systems(Last, board_probe::board_probe);` at the top of
`ReplayViewerPlugin::build`. It does nothing unless `GT_PROBE` is set, and
exits the game on its own once the image is saved:

```sh
cargo build
GT_PROBE=match GT_PROBE_OUT=/tmp/board.png ./target/debug/go-together
GT_PROBE=viewer GT_PROBE_UI=1 GT_PROBE_KEY=1 GT_PROBE_OUT=/tmp/key.png ./target/debug/go-together
```

`GT_PROBE` is `match`, `viewer`, `library`, `settings`, or anything else for
Home. `GT_PROBE_UI` captures the whole 1040x900 window with its interface
instead of a square around the board; `GT_PROBE_AT` seeks the Viewer to a
position in seconds instead of the end; `GT_PROBE_KEY` opens the Viewer's Key.
The `match` mode writes a position straight into `GameState` - every stone
colour beside every other and beside bare wood - and the `library` mode adds
one card with the same position to the in-memory catalog, so neither needs a
saved Replay and neither saves one. Run it from the repository root: the
Viewer and Library modes read a fixture by relative path.

Remove all of it before committing. The source-reading guards in
`src/architecture.rs` would read it as production code, and it writes to
disk.

**Running either probe in a cloud container**, where there is no display
and no GPU, takes two packages beyond the README's build set and a virtual
X server. Learned on 2026-10-03: `libxkbcommon-x11-0`, without which winit
panics before a window exists, and `mesa-vulkan-drivers`, whose software
Vulkan driver (lavapipe) is the only adapter Bevy will find - this build
has no GL backend, so `WGPU_BACKEND=gl` finds nothing. Then
`xvfb-run -a -s "-screen 0 1280x1024x24" ./target/debug/go-together` runs
the game, slowly, and both probes work unchanged. Bevy warns that the
adapter is software; that warning is expected. A run of a few hundred frames with a screenshot on
most of them took about a minute.

```rust
use bevy::app::AppExit;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use super::ReplayViewerState;
use super::geometry::VIEWER_BOARD_CENTER_Y;
use crate::app::AppScreen;
use crate::app::visuals::{BOARD_CENTER_Y, WINDOW_HEIGHT, WINDOW_WIDTH};
use crate::gameplay::{GameState, MatchPhase};
use crate::replay::Replay;
use crate::replay_library::{ReplayCatalogEntry, ReplayLibrary};
use crate::rules::{Cell, Point};

/// A position with every stone colour beside every other and beside bare
/// wood: lone gray stones, gray on the edge and in the corner, gray against
/// black and against white, and star points both covered and clear.
const STONES: [(Cell, &[(usize, usize)]); 3] = [
    (
        Cell::Black,
        &[(1, 6), (2, 6), (1, 5), (5, 2), (6, 3), (7, 7)],
    ),
    (
        Cell::White,
        &[(2, 5), (3, 5), (3, 6), (6, 2), (5, 3), (7, 1)],
    ),
    (
        Cell::Gray,
        &[
            (4, 4),
            (0, 8),
            (0, 0),
            (8, 4),
            (4, 1),
            (4, 2),
            (2, 4),
            (7, 5),
            (7, 6),
            (5, 6),
        ],
    ),
];
const FIXTURE: &str = "tests/fixtures/replays/v6-round-zero.json";
/// Side of the square captured around a board, in logical pixels.
const BOARD_CAPTURE: f32 = 700.0;
/// Rendered at twice the logical size, as on a high-density display.
const SCALE: f32 = 2.0;

pub(crate) fn board_probe(
    mut commands: Commands,
    mut frames: Local<u32>,
    mut target: Local<Option<Handle<Image>>>,
    mut images: ResMut<Assets<Image>>,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut game: ResMut<GameState>,
    mut viewer: ResMut<ReplayViewerState>,
    mut library: ResMut<ReplayLibrary>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(mode) = std::env::var("GT_PROBE") else {
        return;
    };
    let out = std::env::var("GT_PROBE_OUT").expect("GT_PROBE_OUT names the PNG to write");
    let whole_window = std::env::var("GT_PROBE_UI").is_ok();
    *frames += 1;
    let frame = *frames;
    let fixture = || {
        let json = std::fs::read_to_string(FIXTURE).expect("run from the repository root");
        Replay::from_json(&json).expect("the fixture is a valid Replay")
    };

    if frame == 10 {
        match mode.as_str() {
            "match" => next_screen.set(AppScreen::Match),
            "settings" => next_screen.set(AppScreen::ControlMapping),
            "viewer" => {
                viewer.replay_id = Some("probe".into());
                viewer.replay = Some(fixture());
                next_screen.set(AppScreen::ReplayViewer);
            }
            "library" => {
                // In memory only: nothing here is saved.
                let mut entry = ReplayCatalogEntry::from_replay(&fixture());
                entry.label = "Probe position".into();
                for (cell, points) in STONES {
                    for (x, y) in points {
                        entry.final_board.set(Point::new(*x, *y), cell);
                    }
                }
                library.entries.push(entry);
                next_screen.set(AppScreen::ReplayLibrary);
            }
            _ => {}
        }
    }
    if frame == 20 {
        let (width, height, center_y) = if whole_window {
            (WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32, 0.0)
        } else if mode == "viewer" {
            (BOARD_CAPTURE, BOARD_CAPTURE, VIEWER_BOARD_CENTER_Y)
        } else {
            (BOARD_CAPTURE, BOARD_CAPTURE, BOARD_CENTER_Y)
        };
        let mut image = Image::new_target_texture(
            (width * SCALE) as u32,
            (height * SCALE) as u32,
            TextureFormat::Rgba8UnormSrgb,
            None,
        );
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let handle = images.add(image);
        let mut camera = commands.spawn((
            Camera2d,
            Camera {
                order: -1,
                ..default()
            },
            RenderTarget::Image(handle.clone().into()),
            Projection::Orthographic(OrthographicProjection {
                scale: 1.0 / SCALE,
                ..OrthographicProjection::default_2d()
            }),
            Transform::from_xyz(0.0, center_y, 0.0),
        ));
        if whole_window {
            // bevy_ui draws to the default UI camera and lays itself out in
            // that target's own pixels, so it needs both of these to appear
            // at all and to appear at the size of the board it surrounds.
            camera.insert(IsDefaultUiCamera);
            commands.insert_resource(UiScale(SCALE));
        }
        *target = Some(handle);
    }
    if frame >= 20 {
        if mode == "match" {
            game.phase = MatchPhase::Playing;
            for (cell, points) in STONES {
                for (x, y) in points {
                    game.board.set(Point::new(*x, *y), cell);
                }
            }
            game.cursors = [Point::new(4, 4), Point::new(5, 3)];
        } else if let Some(duration) = viewer.replay.as_ref().map(Replay::duration_seconds) {
            let at = std::env::var("GT_PROBE_AT")
                .ok()
                .and_then(|at| at.parse().ok());
            viewer.controller.playing = false;
            viewer.controller.position_seconds = at.unwrap_or(duration);
            viewer.key_open = std::env::var("GT_PROBE_KEY").is_ok();
        }
    }
    if frame == 80 {
        let handle = target.clone().expect("the target was created on frame 20");
        commands
            .spawn(Screenshot::image(handle))
            .observe(save_to_disk(out));
    }
    if frame == 140 {
        exit.write(AppExit::Success);
    }
}
```

**Reaching the Round 0 countdown** needs no frames of its own. The probe's
fixture, `v6-round-zero.json`, shows the Viewer's `3`, `2` and `1` from
0.25 s, 1.25 s and 2.25 s, and `Go!` from 3.25 s, so `GT_PROBE_AT=0.5` is the
`3`. The Match screen's card stays up if the `match` branch writes
`game.phase = MatchPhase::OpeningCountdown` and
`game.countdown_elapsed = MatchDuration::ZERO` (from `crate::clock`) on every
frame in place of the position; it shows the `3`. Both were used on
2026-10-05 to compare the two cards, and a glyph's box - not its node's - is
what that comparison needed: see
[A Text node does not centre its own text](#a-text-node-does-not-centre-its-own-text).

**Reaching the end of a Match** takes one frame of its own and no saved
Replay. Once the screen is up, set `game.phase = MatchPhase::Playing`, write
a position, and call `game.finish(vec![...])` once with the end reasons you
want - `MatchState::finish`, through `GameState`'s `Deref`, which fills
territory and marks the Match ended without the recorder, so nothing is
saved. A position made of two walls scores cleanly: a column of Black at
x = 3 and of White at x = 5 is a 36-36 draw once the territory between
them is filled. Used on 2026-10-07 for the end-of-match captures. **The
probe runs with your real Settings**, so whether the Match timer and the
round panel show follows your own saved preferences rather than the
defaults; write `GamePreferences` (from `crate::settings`) in the probe if a
capture should not depend on them. Or run the binary with `HOME` pointed at
an empty scratch directory (on macOS; `APPDATA` on Windows and
`XDG_CONFIG_HOME` elsewhere, per `app_data_dir` in `src/platform/native.rs`):
the game then reads and writes its settings and Replays there, so every
capture starts from the defaults and an empty Library, and a probe that edits
`MatchSettings` in memory - as the text-size floor's did on 2026-10-07, to
show the longest Match Time caption - cannot reach your real settings file
through a stray key press while its window has focus. Set that `HOME` for
the game's own process alone, not for `cargo run`: cargo and rustup find
their own homes through it as well, and with an empty one rustup sets
about installing a fresh toolchain into the scratch directory (seen on
2026-11-27). Build first and run `./target/debug/go-together` with the
scratch `HOME`, or keep `CARGO_HOME` and `RUSTUP_HOME` pointing at the
real ones.

**Showing the Viewer something no fixture holds.** No Replay under
`tests/fixtures/replays/` carries a Teleport Marker or a selection on a
stone, and the only two marks on one point anywhere in them are the Round 0
selections of (1,0) in `v6-round-zero.json`. So the work that reshaped the
Viewer's board marks (2026-10-01) gave the probe frames of its own. That is
less work than it sounds, because nothing validates a `Replay` once it is in
`ReplayViewerState`: validation runs where one is decoded (`Replay::from_json`,
`Replay::from_value`, the browser catalog) and where the Library saves or
renames, the probe goes around all of them, and the display pass only asks
the `Replay` it is handed for the frame in effect. Every field of `Replay`,
`ReplayEvent` and `ReplayFrame` is `pub(crate)`. So load the fixture for its
metadata and replace its events, one per frame at whole seconds: with the two
frames below, `GT_PROBE_AT=0` shows the first and `GT_PROBE_AT=1` the second.

```rust
fn marks_replay(mut replay: Replay) -> Replay {
    let mut board = Board::default();
    board.set(Point::new(1, 5), Cell::Black);
    board.set(Point::new(3, 3), Cell::White);
    let frame = |markers, selections, cursors| ReplayFrame {
        board: board.clone(),
        cursors,
        teleport_markers: markers,
        selected_moves: selections,
        phase: ReplayPhase::Playing,
        current_round: 1,
        ..default()
    };
    let at = |x, y| Some(Point::new(x, y));
    // Unused cursors are parked in two corners: they are always drawn, and
    // each trails a score badge up and to its right.
    let parked = [Point::new(8, 8), Point::new(8, 0)];
    let frames = vec![
        frame([at(1, 7), at(3, 7)], [at(5, 7), at(7, 7)], parked),
        frame([at(1, 5), at(1, 5)], [at(1, 5), at(3, 3)], parked),
    ];
    let last = frames.len() - 1;
    replay.events = frames
        .into_iter()
        .enumerate()
        .map(|(index, frame)| ReplayEvent {
            at_seconds: index as f64,
            kind: ReplayEventKind::MatchStart,
            frame,
        })
        .collect();
    replay.duration_seconds = last as f64;
    replay
}
```

Two more things, learned on 2026-10-03 when the capture animation needed a
strip of frames rather than one still. The frame clock can be made exact:
inserting `TimeUpdateStrategy::ManualDuration` of one sixtieth of a second
on the probe's first frame makes every frame advance `Time` by exactly
that, whatever the display does, so a screenshot taken `n` frames after a
change shows it exactly `n / 60` seconds in - which is what makes a strip's
timings worth printing. And `Screenshot` saves the frame *before* the one
it is spawned on: the first tile of a strip requested on the frame a
change was made is byte-for-byte the frame before it. Label a strip from
the request frame minus one. Strips timed this way - seventeen tiles of the
capture animation, six of the end hold's bar - came out pixel-identical on
Bevy 0.19.1 and 0.20.0 on 2026-10-27, so neither has moved.

Three things worth knowing when building such frames. The board is redrawn
only when the event in effect changes, so step the position between frames
rather than editing one in place. A frame has one Teleport Marker and, in
ordinary play, one selection per player; a frame whose `phase` is
`ReplayPhase::Opening` draws up to three selections each from
`opening_selections`, and unvalidated it draws stones as well. And a frame
built this way can show what no recorded Match can - a selection on the
other player's stone, say - which is a reason to label such tiles, not to
avoid them: validation does not rule every one of them out of a hand-edited
file either. The probe above needs `ReplayEvent`, `ReplayEventKind`,
`ReplayFrame` and `ReplayPhase` added to its `crate::replay` import and
`Board` to its `crate::rules` one, and `viewer.replay = Some(fixture())`
changed to wrap the fixture. Stepping the position on a timer and saving one
image per frame makes one launch do for every frame, and rendering at a
`SCALE` of 1.0 as well as 2.0 shows what a display that is not high-density
makes of a stroke two pixels wide.

**Driving the game while capturing.** A probe can play the game as well as
pose it. A `KeyboardInput` message written from `First`, mirrored as a
`WindowEvent::KeyboardInput`, goes through Bevy's own input systems, so the
menus, the focus ring, the text fields and both players' Match keys answer
it as they answer a player; a Home-to-Viewer play-through scripted that way
on 2026-10-27 ran unchanged on two Bevy versions, so its frames could be
diffed. The pointer needs one step more. The interface is laid out on the
capture image, not the window, so picking finds a button only if the
pointer's `PointerInput` is located on that image -
`RenderTarget::Image(handle.into()).normalize(None)`, in the image's
pixels - with the window's `CursorMoved` and `MouseButtonInput` sent too,
for the game's own readers of the first and for `ButtonInput<MouseButton>`.
`Screenshot::primary_window()` is no way round this: that day, with the
screen locked, it saved images of all zeros while the image target captured
normally.

**Measuring a whole screen, before and after.** The help-page probe above
measures one gap. A change that resizes text anywhere else wants every box
on the screen, so the two can be diffed: what moved, by how much, and what
did not. Added to the capture probe, these two queries and one frame of
printing do it - the work that made the player headings 24px (2026-10-04)
used them on the Match screen, both Settings binding tabs, the Preferences
tab and the Viewer's transport, and a plain `diff` of the dumps was the
measurement. Boxes are the nodes' own, in logical pixels from the window's
top-left; a text layout's glyph size changes with `UiScale`, so compare
boxes rather than glyphs. A panel that clips needs its own height compared
with its `max_height` as well, since what does not fit in one shrinks rather
than spills - see the Game tab's test above. The text-size floor
(2026-10-07) was measured this way on all four Settings tabs, the Replay
Library and its delete dialog.

```rust
// Beside the probe's other parameters:
//   texts: Query<(&ComputedNode, &UiGlobalTransform, &Text)>,
//   buttons: Query<(&ComputedNode, &UiGlobalTransform), With<Button>>,
// with `bevy::ui::{ComputedNode, UiGlobalTransform}` and
// `bevy::ui_widgets::Button` imported - the prelude's `Button` is
// `bevy_ui`'s deprecated one, which no button in the game carries.
if frame == 60 {
    let rect = |node: &ComputedNode, transform: &UiGlobalTransform| {
        let scale = node.inverse_scale_factor;
        let (centre, size) = (transform.translation * scale, node.size * scale);
        (centre.x - size.x / 2.0, centre.y - size.y / 2.0, size.x, size.y)
    };
    let mut lines = Vec::new();
    for (node, transform, text) in &texts {
        let (left, top, width, height) = rect(node, transform);
        lines.push(format!("TEXT {left:7.1} {top:7.1} {width:7.1} {height:7.1} {:?}", text.0));
    }
    for (node, transform) in &buttons {
        let (left, top, width, height) = rect(node, transform);
        lines.push(format!("BUTTON {left:7.1} {top:7.1} {width:7.1} {height:7.1}"));
    }
    lines.sort();
    lines.iter().for_each(|line| println!("PROBE {line}"));
}
```

The probe's `settings` mode opens the Preferences tab, and the tab a
Settings screen shows is private to `ui::settings`, so reaching another
tab takes a small system there - reading an environment variable and
setting `ControlMappingState::tab` - registered in `SettingsPlugin::build`
and removed with the rest. Two cursors on one point, which no fixture
holds, is a frame built as above with `cursors: [point, point]`.

For a packaged-native smoke test, move the bundle to a directory unrelated to
the checkout, start it from another working directory, and confirm it loads the
adjacent `assets/`. On the machine that built it,
`python3 scripts/machine_paths.py check <bundle or archive>` should also pass:
what is published holds no path of that machine and no name of its user (see
[Checking what is published](DISTRIBUTION.md#checking-what-is-published)).

For the web build:

1. run `trunk serve` and open its printed local URL;
2. confirm the centered canvas keeps its 1040x900 aspect in a narrow iframe and
   fullscreen;
3. wait for the start prompt, click the canvas (or focus it and press a key), and
   confirm the app starts with keyboard focus and no autoplay failure, in
   Chrome, Firefox, and Safari - the three differ in exactly this step (see
   [Starting in a browser](DISTRIBUTION.md#starting-in-a-browser));
4. test a browser gamepad only when the browser reports a standard mapping;
5. change Settings, reload, and confirm they persist when storage is available;
6. complete four short Matches and confirm only the newest three Replays remain;
7. repeat with storage blocked/private and confirm play continues with visible
   failure handling; and
8. serve an extracted `dist/` and confirm all JavaScript, WebAssembly, shell, and
   audio requests are relative and same-origin.

There is no service worker to unregister or offline cache to clear.
