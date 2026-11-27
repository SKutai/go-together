# Replay fixtures

Literal on-disk Replays, one per supported schema version plus seven richer
shapes, exactly as `Replay::to_storage_json` writes them - compact, on one
line each, which is why a diff of one is best read with
`git diff --word-diff` or by parsing it. They exist so that
`tests/headless_replay.rs` can prove, through the crate's public API and
under `cargo test --no-default-features`, that every schema version still
loads, validates, plays back, and re-serializes byte-for-byte.

| File | Built from | What it adds |
| --- | --- | --- |
| `v1.json` .. `v6.json` | `replay::test_support::literal_replay_value(version)` | The minimal three-event Match (start, one cursor move, end) in each version's exact historical shape |
| `v2-legacy-audio.json` | `legacy_audio_replay_v2` in `replay::compat`'s test module (the same v2 shape `replay::validation`'s legacy-audio test builds) | A teleport jump and a Gray round whose sounds must be *derived*, because v2 predates recorded sound events |
| `v6-round-zero.json` | `replay::test_support::round_zero_replay`, plus a recorded sound at time zero (`round_zero_replay_with_sound_at_zero` in the same test module) | A complete Round 0: selections, countdown stages with recorded cues, reveal, Go - and a cue at the very first instant |
| `v6-round-timeout.json` | `replay::test_support::round_timeout_replay` | An enabled round-time rule, a round completed by timeout, and a running round clock |
| `v6-match-timeout.json` | `replay::test_support::match_timeout_replay` | An enabled match-time rule, a `match_time` timeout cause, and a `match_time_limit` end reason |
| `v6-sequential-capture.json` | `replay::test_support::sequential_new_stone_capture_replay` | Both players' locked selections, a sequential resolution, and a nonzero `captures` payload |
| `v6-suicide-timeout.json` | `replay::test_support::suicide_timeout_replay` | Capture points enabled, a one-sided timeout with self-removal credits, and a recorded capture cue |
| `v6-sweeping-reveal.json` | `replay::test_support::sweeping_reveal_replay` | A Round 0 reveal whose sweep removes a zero-liberty corner stone: the reveal frame carries the cleaned board and the sweep's capture credit |

The guard test's fixture list pins each file's ID - the recorder draws a
random UUID - so the same builder writes the same bytes on every machine.
For the seven richer shapes it pins a label as well, naming what the fixture
demonstrates; `v1.json` .. `v6.json` keep the `Schema N` label their builder
gives them. (The default label itself is deterministic: fixtures hand the
recorder a fixed start-time text rather than a local-time-zone rendering.)
`.gitattributes` pins the files to LF line endings for the same reason: the
comparison is byte-for-byte against what the crate writes. The files also
end without a trailing newline, because `serde_json::to_string` writes
none - and, since 2026-09-11, without indentation either: the writer emits
compact JSON, so each of these files is a single line.
`.editorconfig`'s `insert_final_newline = true` applies to them, so an
editor that honors it appends one on save and the guard test then fails:
never hand-edit a fixture, regenerate it.

## Guard and regeneration

`replay::compat::tests::checked_in_fixtures_match_their_builders` (both
build configurations) asserts each file is byte-identical to what its
builder writes today.
If it fails, one of two things happened:

- **A builder was deliberately changed.** Regenerate with
  `GO_TOGETHER_UPDATE_REPLAY_FIXTURES=1 cargo test checked_in_fixtures`,
  review the diff, and commit the new files.
- **The writer changed** - `to_storage_value`, a `#[serde]` attribute, a
  field's default. A file that a shipped build wrote has changed shape, which
  is an on-disk format change (see `CONTRIBUTING.md`'s "Changing persisted
  data"). Do not regenerate to make the test pass; reconsider the change, or
  add a new schema version and a new fixture for it.

A fixture that stops *loading* is the second case by definition.

That second case has been accepted deliberately exactly once, on 2026-09-11,
when `to_storage_json` stopped pretty-printing so that a long Match would fit
under `MAX_REPLAY_JSON_BYTES` (see `docs/ARCHITECTURE.md`'s "Replay size").
What made it acceptable is narrower than it may look, and is the bar to clear
before doing anything like it again: only whitespace changed, so the parsed
JSON *value*, every field, and every schema version were untouched, and files
already on disk kept loading in both directions. It is also why these files
no longer double as proof that the *indented* files players already have
still load - `tests/headless_replay.rs`'s
`a_pretty_printed_replay_file_still_loads_identically` is what covers that
now, by re-indenting each fixture here and asserting it decodes the same.

## Adding a schema version

Add the new version to the `LEGACY_REPLAY_SCHEMA_VERSION..=REPLAY_SCHEMA_VERSION`
loop's expectations in `tests/headless_replay.rs`, regenerate (which writes
the new `vN.json`), and keep every older file unchanged.

Regeneration only produces the right bytes once three per-version ladders
know the new version, so give it its arm in each before regenerating:
`Replay::to_storage_value` in `replay::compat`, which strips the new fields
back out of older Replays so they keep re-serializing byte-for-byte; and
`literal_replay_value` and `assert_storage_schema` in
`replay::test_support`, which build and check each historical shape. Without
them the previous version's `vN.json` would be regenerated carrying the new
fields. The guard test's own fixture list needs no edit: it loops over the
same version range and picks the new file up on its own.
