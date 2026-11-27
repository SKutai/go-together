# Instructions for coding agents

This file is for a coding agent helping someone contribute to Go Together.
It is short on purpose. [CONTRIBUTING.md](CONTRIBUTING.md) is the guide, all
of it applies to you, and nothing here replaces it: these are the things
most easily got wrong here, most with the place that explains them.

- **Run every required check, not a subset.** The commands under
  [Required checks](CONTRIBUTING.md#required-checks) are what CI runs, and a
  pull request cannot be merged until they pass. The ones that get skipped
  are the two `--no-default-features` commands, which build and test the
  headless core without Bevy, and the two `cargo doc` commands with
  `RUSTDOCFLAGS='-D warnings'`, which fail on a broken intra-doc link where
  a plain `cargo doc` at most warns. Neither pair stands in for the rest:
  the headless tests compile no UI code at all.
- **Doc comments are checked code.** A fenced block with no language label
  in a `///` or `//!` comment is a doctest that `cargo test` compiles and
  runs, and `-D warnings` denies every rustdoc lint that warns by default,
  a bare URL and an unbackticked `Vec<u8>` among them. The same section says
  how to write each.
- **Some tests read the crate's own source.** They hold in-game text to
  printable ASCII and a 14px floor, allow a color to be made only in
  `src/presentation.rs`, and check which module may depend on which. Others
  scan whole directories for retired phrasing, and those read the text as it
  is written, so there a forbidden phrase in a comment fails as surely as
  one in a string. Read
  [Tests that read the crate's own source](docs/TESTING.md#tests-that-read-the-crates-own-source)
  before rewording UI copy.
- **A failing check is information.** Fix what it found. Do not reach for
  `#[allow(...)]`, a new exemption or a loosened assertion to get past one;
  where an exemption is the honest answer, CONTRIBUTING.md says what it has
  to state.
- **Persisted names are a compatibility contract.** Serde field and enum
  names, schema versions, Replay IDs, filenames and the browser storage keys
  are what players' existing Settings and Replays are read by. Never rename
  or reinterpret one: add a schema version, a migration and a fixture, as
  [Changing persisted data](CONTRIBUTING.md#changing-persisted-data) sets
  out.
- **Keep the documents true as you go.** `docs/` and the module-level `//!`
  comments carry design reasoning and measured numbers. Correct the claims
  your change makes stale instead of only adding new prose, and do not state
  a number you have not measured.
- **An idea that is not settled stays out of the settled documents.** The
  ones directly under `docs/` describe the game as it is. An idea is raised
  in an issue; one that has been discussed and parked is written down in
  [`docs/ideas/`](docs/ideas/README.md), which nothing may cite as evidence
  of how the game behaves. Do not add a file there as a side effect of an
  unrelated change.
- **Read the code beside your change before writing Bevy from memory.** The
  Bevy version is the one in `Cargo.toml`, and an API remembered from another
  version often does not exist in it.
- **Say what you could not check.** `cargo run` is expected after a change to
  startup, assets, screens, UI or audio. If you cannot open a window or hear
  a sound, say so in the pull request instead of implying it was looked at.

A pull request is kept to one behavior or one maintainability goal, without
unrelated reformatting, and none of its commits is marked `[skip ci]`; the
[Small-change workflow](CONTRIBUTING.md#small-change-workflow) says why,
what the description has to explain, and how it is merged.
