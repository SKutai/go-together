//! Test-only guards that hold this crate's own structure to what
//! [`ALLOWED_EDGES`] and [`DECLARED_CYCLES`] below declare about it.
//!
//! Those two are the checked copy. `docs/ARCHITECTURE.md` describes the same
//! shape in prose, and keeping its table in step is an editorial obligation
//! that nothing here enforces - no test reads that file.
//!
//! Every guard here works the same way: it reads this crate's own source
//! files off disk at test time, rather than being handed a list of them.
//!
//! * [`sources_in`] and [`source_file`] back the negative assertions
//!   scattered through the UI modules - "this phrasing is gone from the
//!   code" - which previously `include_str!`ed a hand-written file list.
//!   A negative assertion cannot fail because of a file nobody listed, so
//!   that list could go stale without any test noticing, and did: the
//!   `replay` list lost `catalog.rs` when the catalog was extracted.
//!   Reading a directory instead means a new sibling is covered the moment
//!   it exists. [`source_file`] also backs one positive check in the
//!   headless core: `replay::measurement`'s guard that its mirrored
//!   `BROWSER_REPLAY_LIMIT` still matches `platform`. That is why it carries
//!   no `#[cfg(feature = "client")]` while [`sources_in`] does.
//! * `tests::module_dependencies_match_the_allowed_edge_table` extracts each
//!   top-level module's `crate::` edges and compares them against
//!   [`ALLOWED_EDGES`], the authoritative, checked copy of the edge list.
//!   `docs/ARCHITECTURE.md`'s "Module ownership and dependency direction"
//!   section says what each module is *for* and deliberately does not
//!   repeat the edges - a second, unchecked transcription of them is
//!   precisely the failure mode this constant exists to remove.
//! * `tests::production_string_literals_are_printable_ascii` reads every
//!   string literal in the crate's production code and holds it to
//!   `CONTRIBUTING.md`'s in-game text rule - printable ASCII, because
//!   Bevy's bundled fallback font draws anything else as a missing-glyph
//!   box. It shares [`lex`] and the `#[cfg(test)]` stripping with the
//!   graph scanner, which is what lets it pass over the doc comments and
//!   test fixtures that name non-ASCII on purpose. Its own doc comment says
//!   where that line is drawn and what falls outside it.
//! * `tests::colours_are_constructed_only_in_presentation` holds every
//!   production file but `presentation.rs` to making no colour of its own,
//!   with the same lexing and `cfg` stripping. It is half of a pair: the
//!   contrast guard in `contrast.rs` checks every colour constant against
//!   the backgrounds it is drawn on, and this is what makes "every colour
//!   constant" mean every colour.
//! * `tests::no_text_is_set_below_the_floor` holds every text size the
//!   production code sets to `CONTRIBUTING.md`'s floor of 14px, by the same
//!   lexing. It reads a size where it is set, following a named constant
//!   to its declaration, because a size it cannot read is reported rather
//!   than assumed - the audit that first found 11px text missed one set
//!   through a constant.
//!
//! # What keeps a source-reading test honest
//!
//! A test that reads source files has a failure mode a normal test does
//! not: if it reads the wrong place, or a filter throws everything away, it
//! keeps passing and now lies. Every entry point here is built so that
//! scanning too little fails:
//!
//! * [`sources_in`] takes the number of files it must find and panics below
//!   it, naming what it did find. The count is a floor, not a manifest - a
//!   new sibling raises coverage silently, which is the point, while
//!   deleting one becomes a visible edit somebody has to make on purpose.
//! * [`ALLOWED_EDGES`] is checked in **both** directions. A declared edge
//!   that no longer appears in the source fails exactly as loudly as an
//!   undeclared edge that does. So a scanner that quietly stopped finding
//!   anything - the failure that would make all of this worthless - reports
//!   itself as a wall of missing edges rather than as a pass.
//! * [`production_source_files`] panics if the walk turns up fewer files
//!   than the crate is known to have, and the printable-ASCII guard built
//!   on it requires a floor of literals as well, so a lexer that classified
//!   everything as a comment would fail rather than find nothing to object
//!   to. The colour guard carries the same kind of floor: it must find the
//!   constructions `presentation.rs` is known to hold before its finding
//!   none elsewhere counts for anything, and the text-size guard must read
//!   [`MINIMUM_FONT_SIZES`] sizes before finding none below the floor
//!   means anything.
//!
//! # `CARGO_MANIFEST_DIR` is a compile-time path
//!
//! [`source_root`] builds an absolute path from `env!("CARGO_MANIFEST_DIR")`,
//! which `cargo` substitutes when this file is *compiled*. It names a real
//! directory when the test binary runs from the checkout it was built in,
//! which is the only way these tests are run (`cargo test`, per
//! `CONTRIBUTING.md`). It is not a runtime guarantee: a test binary copied
//! to another machine, or run after the checkout moved, would find nothing
//! there. That is why every accessor below asserts the path exists instead
//! of treating an empty read as an empty result - the one thing a guard
//! like this must never do is pass because it looked in the wrong place.

use std::collections::{BTreeMap, BTreeSet};
use std::iter::Peekable;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::str::Chars;

/// This crate's `src/` directory.
///
/// See the module docs for why this is a compile-time path and what that
/// does and does not promise.
fn source_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(
        root.is_dir(),
        "{} is not a directory: these guards read the crate's own sources from \
         the checkout they were compiled in, and cannot run anywhere else",
        root.display()
    );
    root
}

/// Every `.rs` file under `directory`, recursively, in sorted order.
fn rust_files_under(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = std::fs::read_dir(&current)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", current.display()));
        for entry in entries {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The concatenated text of every `.rs` file under `src/<relative_dir>`,
/// recursively.
///
/// `at_least` is how many files the caller expects the walk to turn up, and
/// finding fewer is a failure. See the module docs for why the count is a
/// floor rather than a list.
#[cfg(feature = "client")]
pub(crate) fn sources_in(relative_dir: &str, at_least: usize) -> String {
    let directory = source_root().join(relative_dir);
    assert!(
        directory.is_dir(),
        "src/{relative_dir} is not a directory - a scan that finds nothing \
         would pass every negative assertion in it"
    );
    let files = rust_files_under(&directory);
    assert!(
        files.len() >= at_least,
        "expected at least {at_least} source files under src/{relative_dir}, found {}: {:?}",
        files.len(),
        file_names(&files)
    );
    files.iter().map(|path| read(path)).collect::<String>()
}

/// The text of one source file, named relative to `src/`.
pub(crate) fn source_file(relative_path: &str) -> String {
    read(&source_root().join(relative_path))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

#[cfg(feature = "client")]
fn file_names(files: &[PathBuf]) -> Vec<String> {
    files
        .iter()
        .map(|path| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The module dependency graph
// ---------------------------------------------------------------------------

/// Which top-level modules each top-level module may name.
///
/// This is the authoritative, checked copy of the edge list behind
/// `docs/ARCHITECTURE.md`'s "Module ownership and dependency direction"
/// section, which describes what each module is for and deliberately does
/// not repeat the edges themselves. An entry means "files under
/// `src/<module>/` may write `crate::<target>`" - nothing finer. The
/// rows are deliberately per *top-level module* and not per file, so that
/// splitting a module into a directory, or moving code between its own
/// files, does not touch this table.
///
/// Edges here are production edges only: `#[cfg(test)]` code is stripped
/// before the scan, because test code legitimately reaches for fixtures the
/// production graph forbids, and doc comments are stripped because a doc
/// link points wherever the prose needs to point.
///
/// **Both halves are checked.** An edge in the source that is missing here
/// fails, and an edge here that is no longer in the source fails too. The
/// second direction is what stops this table from accumulating permissions
/// nobody needs any more, and what makes a broken scanner announce itself.
const ALLOWED_EDGES: &[ModuleRow] = &[
    // Headless core. Everything below `authority` is leaf-ward of it, and
    // none of it may name a `client` module; `cargo build
    // --no-default-features` is what actually enforces that half.
    ModuleRow::new("clock", &[]),
    ModuleRow::new("match_settings", &[]),
    ModuleRow::new("rules", &["clock"]),
    ModuleRow::new("cue", &["rules"]),
    ModuleRow::new("opening", &["clock", "rules"]),
    ModuleRow::new(
        "replay",
        &["clock", "cue", "match_settings", "opening", "rules"],
    ),
    // `authority` -> `replay` is an ordinary core-to-core edge; see the
    // prose under the table in `docs/ARCHITECTURE.md` for why the
    // `EndReason` dependency is not a layering violation.
    ModuleRow::new(
        "authority",
        &["clock", "match_settings", "opening", "replay", "rules"],
    ),
    ModuleRow::new("view", &["authority", "opening", "replay", "rules"]),
    // Client. `diagnostics` is a leaf on purpose: it turns typed errors into
    // player-facing notices and must not know which subsystem produced one.
    ModuleRow::new("diagnostics", &[]),
    ModuleRow::new("presentation", &["rules"]),
    ModuleRow::new("input", &["presentation", "rules"]),
    ModuleRow::new("audio", &["cue", "settings"]),
    ModuleRow::new(
        "settings",
        &["input", "match_settings", "platform", "rules"],
    ),
    ModuleRow::new("platform", &["replay", "replay_library", "settings"]),
    ModuleRow::new("replay_library", &["platform", "replay", "rules"]),
    ModuleRow::new(
        "gameplay",
        &[
            "app",
            "audio",
            "authority",
            "clock",
            "diagnostics",
            "input",
            "match_settings",
            "opening",
            "platform",
            "replay",
            "replay_library",
            "rules",
        ],
    ),
    ModuleRow::new(
        "ui",
        &[
            "app",
            "audio",
            "diagnostics",
            "gameplay",
            "input",
            "match_settings",
            "opening",
            "platform",
            "presentation",
            "replay",
            "replay_library",
            "rules",
            "settings",
        ],
    ),
    ModuleRow::new(
        "app",
        &[
            "audio",
            "diagnostics",
            "gameplay",
            "input",
            "platform",
            "presentation",
            "replay_library",
            "rules",
            "settings",
            "ui",
        ],
    ),
];

/// Every pair of modules that name each other, and why that is allowed.
///
/// Rust permits module cycles, so there is no compiler error to lean on
/// here and never was: these four have always been cycles and have always
/// compiled. What this list buys is that a *fifth* cannot appear without
/// somebody writing down why - which is the whole difference between a
/// documented shape and an accident.
///
/// A reason is prose, so no test can judge it. What the test can do, and
/// does, is refuse a cycle that has no entry at all.
const DECLARED_CYCLES: &[DeclaredCycle] = &[
    DeclaredCycle {
        between: ("app", "gameplay"),
        reason: "Composition root and feature plugin. `app` names \
                 `GameplayPlugin` because registering it is what a \
                 composition root does; `gameplay` names `AppScreen` and \
                 `AppSystems` because the screen state machine and the \
                 frame-order vocabulary are `app`'s to own and every plugin \
                 schedules against them. Breaking this needs a third module \
                 holding the shared vocabulary, which buys nothing: the \
                 cycle is between a root and the parts it composes, and \
                 cannot produce a surprising transitive path.",
    },
    DeclaredCycle {
        between: ("app", "ui"),
        reason: "The same composition-root shape as `app`/`gameplay`. \
                 `app` registers `UiPlugin`; every screen names \
                 `AppScreen`, `AppSystems`, and the window dimensions \
                 `app::visuals` owns.",
    },
    DeclaredCycle {
        between: ("platform", "replay_library"),
        reason: "Port-and-adapter. `platform` defines `ReplayStorage` next \
                 to the native and browser adapters that implement it, and \
                 the port speaks `ReplayCatalogEntry`, which `replay_library` \
                 owns because projecting, searching, and sorting catalog \
                 entries is domain work and not storage work. \
                 `replay_library` names `platform` back for the \
                 `replay_storage()` factory that picks an adapter and for \
                 `format_local_timestamp`, both host capabilities. Moving \
                 the trait into `replay_library` does not remove this edge - \
                 see the analysis under the table in `docs/ARCHITECTURE.md`.",
    },
    DeclaredCycle {
        between: ("platform", "settings"),
        reason: "The same port-and-adapter shape as the `replay_library` \
                 entry above. Both adapters take `MAX_SETTINGS_JSON_BYTES` \
                 from `settings`, which is domain policy that \
                 `ControlsConfig::from_json` enforces and an adapter \
                 re-checks as defence in depth - the port speaking its \
                 domain's vocabulary, not a leak. `settings` names \
                 `platform` back for the `settings_storage()` factory that \
                 picks an adapter, a host capability. The one genuine wart \
                 here, the native file name `CONFIG_FILE_NAME` parked in \
                 `settings`, is gone: it lives in `platform::native` with \
                 the directory it names. See the analysis under the table \
                 in `docs/ARCHITECTURE.md`.",
    },
];

struct ModuleRow {
    module: &'static str,
    depends_on: &'static [&'static str],
}

impl ModuleRow {
    const fn new(module: &'static str, depends_on: &'static [&'static str]) -> Self {
        Self { module, depends_on }
    }
}

struct DeclaredCycle {
    between: (&'static str, &'static str),
    reason: &'static str,
}

/// Fewer source files than this means the walk is reading the wrong place.
///
/// A floor, like [`sources_in`]'s: new files raise coverage silently, and
/// removing enough of them to trip this is a deliberate edit.
const MINIMUM_SCANNED_FILES: usize = 60;

/// Every production `.rs` file in the crate, in sorted order.
///
/// Left out are the files whose `mod` declaration is behind a `cfg` that
/// only holds under `test` - the `test_support` modules,
/// `rules::properties`, `replay::measurement`, `gameplay::save_timing`,
/// `contrast`, and this module. Those are found by reading the declarations
/// rather than by
/// matching names, so a new one needs no edit here. `lib.rs` and `main.rs`
/// are production code and are included; [`module_source_files`] is the
/// view that drops them.
///
/// `platform::web` and `platform::browser_storage` are included even though
/// neither is ordinary native production code: `web` compiles only for
/// `wasm32`, and `browser_storage` only for `wasm32` or under `test`. This
/// reads them as text, which is the point - nothing in a native check set
/// looks at `web` at all.
fn production_source_files() -> Vec<PathBuf> {
    let root = source_root();
    let all = rust_files_under(&root);
    assert!(
        all.len() >= MINIMUM_SCANNED_FILES,
        "found only {} source files under {}; expected at least \
         {MINIMUM_SCANNED_FILES}",
        all.len(),
        root.display()
    );

    let test_only = test_only_modules(&all);
    all.into_iter()
        .filter(|path| !test_only.contains(path))
        .collect()
}

/// [`production_source_files`], each paired with its top-level module.
/// `lib.rs` and `main.rs` belong to no module and are skipped.
fn module_source_files() -> Vec<(String, PathBuf)> {
    let root = source_root();
    production_source_files()
        .into_iter()
        .filter_map(|path| top_level_module(&root, &path).map(|module| (module, path)))
        .collect()
}

/// The files whose `mod` declaration sits behind a `cfg` that requires
/// `test`.
fn test_only_modules(all: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut test_only = BTreeSet::new();
    for path in all {
        let source = strip_comments_and_strings(&read(path));
        let directory = module_directory(path);
        for (predicate, name) in cfg_gated_module_declarations(&source) {
            if cfg_requires_test(&predicate) {
                test_only.insert(directory.join(format!("{name}.rs")));
                test_only.insert(directory.join(&name).join("mod.rs"));
            }
        }
    }
    test_only
}

/// Where a file's child modules live: beside `mod.rs`/`lib.rs`/`main.rs`, or
/// in a directory named after any other file.
fn module_directory(path: &Path) -> PathBuf {
    let parent = path
        .parent()
        .expect("a source file has a parent")
        .to_path_buf();
    match path.file_name().and_then(|name| name.to_str()) {
        Some("mod.rs" | "lib.rs" | "main.rs") => parent,
        Some(name) => parent.join(name.trim_end_matches(".rs")),
        None => parent,
    }
}

/// `(predicate, module name)` for every `#[cfg(...)] mod name;` in `source`.
fn cfg_gated_module_declarations(source: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    while let Some(offset) = source[index..].find("#[cfg(") {
        let open = index + offset + "#[cfg".len();
        let Some(close) = matching_paren(bytes, open) else {
            break;
        };
        let predicate = source[open + 1..close].to_string();
        let rest = source[close + 1..].trim_start_matches([']', ' ', '\t', '\r', '\n']);
        let rest = rest
            .trim_start_matches("pub")
            .trim_start()
            .trim_start_matches('(')
            .trim_start_matches("crate")
            .trim_start_matches(')')
            .trim_start();
        if let Some(after_mod) = rest.strip_prefix("mod ") {
            let name: String = after_mod
                .trim_start()
                .chars()
                .take_while(|character| character.is_alphanumeric() || *character == '_')
                .collect();
            if !name.is_empty()
                && after_mod.trim_start()[name.len()..]
                    .trim_start()
                    .starts_with(';')
            {
                found.push((predicate, name));
            }
        }
        index = close + 1;
    }
    found
}

/// Whether a `cfg` predicate can only hold in a test build.
///
/// `test` and `all(..., test)` can only; `any(..., test)` can hold without
/// it, which is why `#[cfg(any(feature = "client", test))] mod recording` is
/// production code and `#[cfg(test)] mod measurement` is not.
fn cfg_requires_test(predicate: &str) -> bool {
    let predicate = predicate.trim();
    if predicate == "test" {
        return true;
    }
    let Some((combinator, body)) = split_combinator(predicate) else {
        return false;
    };
    let parts = split_top_level_commas(body);
    match combinator {
        "all" => parts.iter().any(|part| cfg_requires_test(part)),
        "any" => !parts.is_empty() && parts.iter().all(|part| cfg_requires_test(part)),
        _ => false,
    }
}

fn split_combinator(predicate: &str) -> Option<(&str, &str)> {
    let open = predicate.find('(')?;
    if !predicate.ends_with(')') {
        return None;
    }
    Some((
        predicate[..open].trim(),
        &predicate[open + 1..predicate.len() - 1],
    ))
}

/// `body` split on the commas that are not inside a nested group.
///
/// Both bracket kinds count towards the depth: `cfg` predicates nest with
/// parentheses and `use` groups with braces, and one counter serves both
/// because neither ever contains the other.
fn split_top_level_commas(body: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for character in body.chars() {
        match character {
            '(' | '{' => {
                depth += 1;
                current.push(character);
            }
            ')' | '}' => {
                depth = depth.saturating_sub(1);
                current.push(character);
            }
            ',' if depth == 0 => parts.push(std::mem::take(&mut current)),
            _ => current.push(character),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

fn matching_paren(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// The index of the `}` closing the `{` at `open`, or `None` if the braces
/// never balance. `bytes[open]` must be `{`.
fn matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Which top-level module a file belongs to, or `None` for `lib.rs` and
/// `main.rs`.
fn top_level_module(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let first = relative.components().next()?.as_os_str().to_str()?;
    if path.parent() == Some(root) {
        let stem = first.strip_suffix(".rs")?;
        (stem != "lib" && stem != "main").then(|| stem.to_string())
    } else {
        Some(first.to_string())
    }
}

/// What a run of source text is, as far as the guards here need to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lexeme {
    Code,
    /// A line, block, or doc comment. Nested block comments are one run.
    Comment,
    /// A string literal with its delimiters: `"..."`, `r#"..."#`, and the
    /// `b`-prefixed byte forms. A raw string's run starts at its `r` or
    /// `br`; a plain byte string's `b` is left in the code run before it,
    /// because the lexer only has to recognise the `r` to find the string's
    /// end.
    StringLiteral,
    /// A char literal with its quotes. A `'` that opens a lifetime instead
    /// stays in the code run.
    CharLiteral,
}

/// `source` cut into runs of code, comments, string literals, and char
/// literals: in order, without gaps, as byte ranges into `source`.
///
/// This is the one lexer behind every guard in this module. It knows just
/// enough Rust to find where a comment or a literal ends and nothing more:
/// `'"'` and `'/'` must not open a string or a comment, `&'a str` must not
/// open a char literal, a `//` inside a string is text, and the `r` in
/// `error` does not start a raw string. Consecutive code characters are one
/// run; every other kind is one run per comment or literal.
fn lex(source: &str) -> Vec<(Lexeme, Range<usize>)> {
    let mut lexemes: Vec<(Lexeme, Range<usize>)> = Vec::new();
    let mut index = 0;
    while index < source.len() {
        let rest = &source[index..];
        let (kind, length) = if rest.starts_with("//") {
            (Lexeme::Comment, rest.find('\n').unwrap_or(rest.len()))
        } else if rest.starts_with("/*") {
            (Lexeme::Comment, block_comment_length(rest))
        } else if let Some(length) = raw_string_length(source, index) {
            (Lexeme::StringLiteral, length)
        } else if rest.starts_with('"') {
            (Lexeme::StringLiteral, quoted_string_length(rest))
        } else if let Some(length) = char_literal_length(rest) {
            (Lexeme::CharLiteral, length)
        } else {
            let character = rest.chars().next().expect("a non-empty remainder");
            (Lexeme::Code, character.len_utf8())
        };
        match lexemes.last_mut() {
            Some((Lexeme::Code, range)) if kind == Lexeme::Code => range.end += length,
            _ => lexemes.push((kind, index..index + length)),
        }
        index += length;
    }
    lexemes
}

/// Replaces comments and string and char literals with nothing, so that
/// what is left is code.
///
/// Doc comments are the reason this exists: `[`crate::ui`]` in a `//!` on a
/// core module is a link, not a dependency, and counting those would make
/// the graph nonsense in exactly the direction that matters. String literals
/// go too, so that a `cfg` predicate's `"client"` cannot be mistaken for a
/// bare identifier and a JSON fixture's braces cannot desynchronise the
/// `cfg`-stripping pass that runs after this one.
fn strip_comments_and_strings(source: &str) -> String {
    lex(source)
        .into_iter()
        .filter(|(kind, _)| *kind == Lexeme::Code)
        .map(|(_, range)| &source[range])
        .collect()
}

/// The same view as [`strip_comments_and_strings`], but with every comment
/// and literal replaced by spaces of the same byte length (newlines kept)
/// instead of removed, so that a byte offset into the result is the same
/// offset into `source`. The printable-ASCII guard needs that to map the
/// test-only items it finds in the code back onto the literals beside them.
fn blank_comments_and_strings(source: &str, lexemes: &[(Lexeme, Range<usize>)]) -> String {
    let mut blanked = String::with_capacity(source.len());
    for (kind, range) in lexemes {
        let text = &source[range.clone()];
        if *kind == Lexeme::Code {
            blanked.push_str(text);
        } else {
            blanked.extend(
                text.bytes()
                    .map(|byte| if byte == b'\n' { '\n' } else { ' ' }),
            );
        }
    }
    blanked
}

fn block_comment_length(rest: &str) -> usize {
    let bytes = rest.as_bytes();
    let mut depth = 0usize;
    let mut index = 0;
    while index < bytes.len() {
        if rest[index..].starts_with("/*") {
            depth += 1;
            index += 2;
        } else if rest[index..].starts_with("*/") {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return index;
            }
        } else {
            index += 1;
        }
    }
    rest.len()
}

/// The length of a raw string starting at `index`, if one starts there.
///
/// `r"..."`, `r#"..."#`, and the `b`-prefixed byte forms. The check that the
/// preceding character is not an identifier character is what stops the `r`
/// in `for` or `error` from opening one.
fn raw_string_length(source: &str, index: usize) -> Option<usize> {
    if source[..index]
        .chars()
        .next_back()
        .is_some_and(|character| character.is_alphanumeric() || character == '_')
    {
        return None;
    }
    let rest = &source[index..];
    let after_prefix = rest.strip_prefix("br").or_else(|| rest.strip_prefix('r'))?;
    let hashes = after_prefix.len() - after_prefix.trim_start_matches('#').len();
    let body = after_prefix[hashes..].strip_prefix('"')?;
    let terminator = format!("\"{}", "#".repeat(hashes));
    let end = body
        .find(&terminator)
        .map_or(body.len(), |at| at + terminator.len());
    Some(rest.len() - after_prefix.len() + hashes + 1 + end)
}

fn quoted_string_length(rest: &str) -> usize {
    let bytes = rest.as_bytes();
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    rest.len()
}

/// The length of a char literal, if `rest` starts with one rather than a
/// lifetime.
///
/// `'/'` and `'"'` are the cases that matter: left alone they would open a
/// comment or a string. `&'a str` and `'static` must stay untouched, so a
/// `'` that is not followed by a one-character-then-quote or an escape is
/// treated as a lifetime.
fn char_literal_length(rest: &str) -> Option<usize> {
    let mut characters = rest.chars();
    if characters.next()? != '\'' {
        return None;
    }
    let first = characters.next()?;
    if first == '\\' {
        return rest[2..].find('\'').map(|at| at + 3);
    }
    (characters.next()? == '\'').then(|| 2 + first.len_utf8())
}

/// Removes every item behind a `cfg` that only holds under `test`.
///
/// Runs on already-stripped code, so the braces and semicolons it matches
/// cannot be inside a comment or a string.
fn strip_test_only_items(source: &str) -> String {
    let mut kept = String::with_capacity(source.len());
    let mut index = 0;
    for range in test_only_item_ranges(source) {
        kept.push_str(&source[index..range.start]);
        index = range.end;
    }
    kept.push_str(&source[index..]);
    kept
}

/// The byte range of every item behind a `cfg` that only holds under
/// `test`, attribute included, in source order. An item nested inside one
/// is covered by the outer range and not listed again.
///
/// Runs on stripped or blanked code - [`strip_comments_and_strings`] or
/// [`blank_comments_and_strings`] - so the braces and semicolons it matches
/// cannot be inside a comment or a string.
fn test_only_item_ranges(source: &str) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;
    while let Some(offset) = source[index..].find("#[cfg(") {
        let attribute_start = index + offset;
        let open = attribute_start + "#[cfg".len();
        let Some(close) = matching_paren(bytes, open) else {
            break;
        };
        let end_of_attribute = source[close..]
            .find(']')
            .map_or(bytes.len(), |at| close + at + 1);
        if cfg_requires_test(&source[open + 1..close]) {
            let end = end_of_item(source, end_of_attribute);
            ranges.push(attribute_start..end);
            index = end;
        } else {
            index = end_of_attribute;
        }
    }
    ranges
}

/// Where the item that an attribute is attached to ends: the first `;` at
/// brace depth zero, or the close of its first brace-delimited body.
/// Further attributes in between are skipped over.
fn end_of_item(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let mut index = start;
    loop {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if !source[index..].starts_with("#[") {
            break;
        }
        let mut depth = 0usize;
        while index < bytes.len() {
            match bytes[index] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        index += 1;
                        break;
                    }
                }
                _ => {}
            }
            index += 1;
        }
    }
    let mut depth = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            b';' if depth == 0 => return index + 1,
            _ => {}
        }
        index += 1;
    }
    bytes.len()
}

/// Every `crate::<module>` named in `source`, including the members of a
/// `crate::{a, b}` group and of a nested one such as
/// `crate::{rules::{Board, Player}, clock}`.
///
/// A group runs to the brace matching the one that opened it, not to the
/// first `}` in it, and [`split_top_level_commas`] counts braces as well as
/// parentheses. So a nested group contributes only its own head - `rules`
/// above - and every sibling after it is still seen. Getting this wrong
/// under-reports silently: an edge the source really has would simply go
/// unnoticed, and the guard would pass without it.
fn crate_paths(source: &str) -> Vec<String> {
    let mut named = Vec::new();
    let mut index = 0;
    while let Some(offset) = source[index..].find("crate::") {
        let at = index + offset;
        let preceded_by_identifier = source[..at]
            .chars()
            .next_back()
            .is_some_and(|character| character.is_alphanumeric() || character == '_');
        index = at + "crate::".len();
        if preceded_by_identifier {
            continue;
        }
        let rest = source[index..].trim_start();
        if rest.starts_with('{') {
            let close = matching_brace(rest.as_bytes(), 0).unwrap_or(rest.len());
            for member in split_top_level_commas(&rest[1..close]) {
                named.extend(leading_identifier(member.trim()));
            }
        } else {
            named.extend(leading_identifier(rest));
        }
    }
    named
}

fn leading_identifier(text: &str) -> Option<String> {
    let identifier: String = text
        .chars()
        .take_while(|character| character.is_alphanumeric() || *character == '_')
        .collect();
    (!identifier.is_empty()).then_some(identifier)
}

/// The crate's production module graph: module -> target -> the files that
/// name it.
fn module_graph() -> BTreeMap<String, BTreeMap<String, BTreeSet<String>>> {
    let root = source_root();
    let mut graph: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    for (module, path) in module_source_files() {
        let code = strip_test_only_items(&strip_comments_and_strings(&read(&path)));
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        let targets = graph.entry(module.clone()).or_default();
        for named in crate_paths(&code) {
            if named != module {
                targets.entry(named).or_default().insert(relative.clone());
            }
        }
    }
    graph
}

// ---------------------------------------------------------------------------
// The printable-ASCII rule for in-game text
// ---------------------------------------------------------------------------

/// Fewer string literals than this across the crate's production code means
/// the lexer or the `cfg` stripping is throwing away what it should be
/// reading. A floor, like [`MINIMUM_SCANNED_FILES`]: the crate had close to
/// eight hundred when this was written, and new copy raises the count
/// silently.
const MINIMUM_PRODUCTION_STRING_LITERALS: usize = 400;

/// Every string literal in `source` outside the items a test-only `cfg`
/// gates, as `(line number, literal text)`, delimiters kept.
///
/// Comments never count, so a doc comment may show a reader any character
/// it likes; and a `#[cfg(test)]` module may hold whatever fixtures its
/// tests need. Everything else is production code. The `cfg` stripping is
/// [`test_only_item_ranges`], the same pass the module-dependency guard
/// uses, run over a blanked copy of the code so that the ranges it returns
/// are offsets into the original.
fn production_string_literals(source: &str) -> Vec<(usize, &str)> {
    let lexemes = lex(source);
    let test_only = test_only_item_ranges(&blank_comments_and_strings(source, &lexemes));
    lexemes
        .iter()
        .filter(|(kind, _)| *kind == Lexeme::StringLiteral)
        .filter(|(_, range)| !test_only.iter().any(|item| item.contains(&range.start)))
        .map(|(_, range)| (line_number(source, range.start), &source[range.clone()]))
        .collect()
}

/// The one-based line on which byte `offset` of `source` falls.
fn line_number(source: &str, offset: usize) -> usize {
    source[..offset].matches('\n').count() + 1
}

/// Whether Bevy's bundled fallback font can draw `character`: printable
/// ASCII, or a newline, which is layout rather than a glyph and which the
/// multi-line help pages depend on. The same predicate `ui::hud`'s tests
/// apply to the generated return-home instruction.
fn font_can_draw(character: char) -> bool {
    character == '\n' || (' '..='~').contains(&character)
}

/// The first character in a string literal that [`font_can_draw`] cannot,
/// or `None` if the literal is printable ASCII throughout.
///
/// `literal` is the literal as written, delimiters included. In an ordinary
/// string the escapes are decoded first, so `"\u{2014}"` fails exactly as
/// a bare em dash would; a raw string has none to decode. A line
/// continuation - a backslash before a newline - contributes nothing, as in
/// Rust. Byte strings are read by the same rule: `b"\xff"` fails.
fn first_undrawable(literal: &str) -> Option<char> {
    let has_escapes = literal.starts_with('"');
    let mut characters = literal.chars().peekable();
    while let Some(character) = characters.next() {
        let decoded = if has_escapes && character == '\\' {
            match decode_escape(&mut characters) {
                Some(decoded) => decoded,
                None => continue,
            }
        } else {
            character
        };
        if !font_can_draw(decoded) {
            return Some(decoded);
        }
    }
    None
}

/// Decodes the escape whose backslash the caller has just consumed, as
/// `rustc` would: `None` for a line continuation, which contributes
/// nothing, and the Unicode replacement character for anything malformed,
/// which `rustc` would have refused to compile and which is therefore only
/// reachable from a synthetic sample - it fails the guard rather than being
/// reasoned about.
fn decode_escape(characters: &mut Peekable<Chars<'_>>) -> Option<char> {
    const MALFORMED: char = '\u{fffd}';
    let from_hex = |hex: String| {
        u32::from_str_radix(&hex, 16)
            .ok()
            .and_then(char::from_u32)
            .unwrap_or(MALFORMED)
    };
    match characters.next()? {
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        '0' => Some('\0'),
        'x' => Some(from_hex(characters.by_ref().take(2).collect())),
        'u' => {
            if characters.next() != Some('{') {
                return Some(MALFORMED);
            }
            let hex: String = characters
                .by_ref()
                .take_while(|character| *character != '}')
                .filter(|character| *character != '_')
                .collect();
            Some(from_hex(hex))
        }
        '\n' | '\r' => {
            while characters
                .next_if(|character| character.is_ascii_whitespace())
                .is_some()
            {}
            None
        }
        other => Some(other),
    }
}

/// The first line of `literal`, cut to a width that keeps a failure
/// message readable when the literal is a whole help page.
fn excerpt(literal: &str) -> String {
    const WIDTH: usize = 72;
    let first_line = literal.lines().next().unwrap_or_default();
    match first_line.char_indices().nth(WIDTH) {
        Some((cut, _)) => format!("{}...", &first_line[..cut]),
        None if first_line.len() < literal.len() => format!("{first_line}..."),
        None => first_line.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Colours are made in one file
// ---------------------------------------------------------------------------

/// The one file allowed to construct a colour, relative to `src/`.
const COLOUR_FILE: &str = "presentation.rs";

/// How many colour constructions [`COLOUR_FILE`] must be seen to hold, so
/// that a scan which stopped recognising a constructor fails instead of
/// finding nothing to object to anywhere. A floor, like
/// [`MINIMUM_SCANNED_FILES`]: the file held over forty when this was
/// written.
const MINIMUM_COLOUR_CONSTRUCTIONS: usize = 40;

/// The other types a colour can be built from, and the module of named CSS
/// colours. None is used in production outside [`COLOUR_FILE`] today; each
/// is here so that reaching for one is the same failure as writing
/// `Color::srgb` inline.
const COLOUR_TYPES: &[&str] = &[
    "Srgba",
    "LinearRgba",
    "Hsla",
    "Hsva",
    "Hwba",
    "Laba",
    "Lcha",
    "Oklaba",
    "Oklcha",
    "Xyza",
    "palettes",
];

/// The methods that derive one colour from another, or change one in place.
/// A translucent, lightened or hue-shifted copy of a named colour is a new
/// colour as far as contrast is concerned, so it has to be named too. These
/// are every such operation `bevy_color` gives `Color`.
const COLOUR_DERIVATIONS: &[&str] = &[
    "with_alpha",
    "set_alpha",
    "with_luminance",
    "with_hue",
    "set_hue",
    "rotate_hue",
    "with_saturation",
    "set_saturation",
    "lighter",
    "darker",
    "mix",
    "mix_assign",
];

/// The named colours Bevy's colour-carrying components offer as associated
/// constants - `TextColor::WHITE` and the like - which make a colour without
/// the word `Color::` appearing.
const WRAPPED_COLOURS: &[&str] = &["WHITE", "BLACK"];

/// Every place production code in `source` makes a colour, as `(line
/// number, what was written)`.
///
/// "Makes" means four things: any associated item of `Color` other than
/// `Color::NONE` - a constructor such as `Color::srgb`, or a named colour
/// such as `Color::WHITE`; one of [`WRAPPED_COLOURS`] on a type whose name
/// ends in `Color`, such as `TextColor::WHITE`; any mention of one of
/// [`COLOUR_TYPES`]; and any call of one of [`COLOUR_DERIVATIONS`], as a
/// method or by its trait path. `Color::NONE` is let through because fully
/// transparent is the absence of a colour, which is how the focus ring
/// parks its outline.
///
/// Comments, string literals and test-only items are not read, by the same
/// passes the printable-ASCII guard uses, so a test may assert against
/// `Color::srgb(...)` and a doc comment may name it.
///
/// # What it does not see
///
/// This reads words, so it catches a colour being *written* and cannot
/// catch one that arrives unwritten:
///
/// * **A default.** A `Text` spawned with no `TextColor` is white, and
///   `Color::default()` would be reported but `..default()` on a struct
///   that holds a colour is not. No production code relies on one today.
/// * **A renamed type.** `use bevy::prelude::Color as Paint` or a `type`
///   alias hides the word this looks for.
/// * **A colour computed from channels** it never sees named, such as an
///   array converted with `.into()`.
///
/// It is a guard against drift, not against someone working around it: the
/// ordinary way to add a colour to a screen is to write one of the things
/// above, and that is what fails.
pub(crate) fn colour_constructions(source: &str) -> Vec<(usize, String)> {
    let lexemes = lex(source);
    let code = blank_comments_and_strings(source, &lexemes);
    let test_only = test_only_item_ranges(&code);
    let is_identifier = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let bytes = code.as_bytes();
    let identifier_at = |start: usize| {
        let length = bytes[start..]
            .iter()
            .take_while(|byte| is_identifier(**byte))
            .count();
        &code[start..start + length]
    };

    let mut found = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if !is_identifier(bytes[index]) || (index > 0 && is_identifier(bytes[index - 1])) {
            index += 1;
            continue;
        }
        let word = identifier_at(index);
        let after = index + word.len();
        let follows = |prefix: &str| code[..index].ends_with(prefix);
        let written = if word == "Color" && code[after..].starts_with("::") {
            let item = identifier_at(after + 2);
            (item != "NONE").then(|| format!("Color::{item}"))
        } else if word.ends_with("Color") && code[after..].starts_with("::") {
            let item = identifier_at(after + 2);
            WRAPPED_COLOURS
                .contains(&item)
                .then(|| format!("{word}::{item}"))
        } else if COLOUR_TYPES.contains(&word) {
            Some(word.to_string())
        } else if COLOUR_DERIVATIONS.contains(&word)
            && (follows(".") || follows("::"))
            && code[after..].starts_with('(')
        {
            Some(format!(".{word}("))
        } else {
            None
        };
        if let Some(written) = written
            && !test_only.iter().any(|item| item.contains(&index))
        {
            found.push((line_number(source, index), written));
        }
        index = after;
    }
    found
}

// ---------------------------------------------------------------------------
// The text-size floor
// ---------------------------------------------------------------------------

/// The smallest text the game may draw, in logical pixels: the floor
/// `CONTRIBUTING.md` states under "UI and accessibility". Every text was
/// raised to it on 2026-10-07 from as little as 11px.
const TEXT_FLOOR_PX: f32 = 14.0;

/// How many text sizes production code must be seen to set, so that a scan
/// which stopped recognising `FontSize::Px` fails instead of finding nothing
/// below the floor. A floor, like [`MINIMUM_SCANNED_FILES`]: the crate set
/// over eighty when this was written.
const MINIMUM_FONT_SIZES: usize = 80;

/// The `TextFont` builders that take a size, which are ways of setting one
/// without writing `FontSize`.
const FONT_SIZE_BUILDERS: &[&str] = &["from_font_size", "with_font_size"];

/// One place production code sets a text size, as it was written.
#[derive(Debug, PartialEq)]
enum FontSizeWritten {
    /// `FontSize::Px(...)`, holding the argument as written, trimmed.
    Px(String),
    /// Any other way of setting a size - another unit, a conversion into
    /// one, or a `TextFont` builder - holding what was written.
    Other(String),
}

/// The words of `code` that start where an identifier starts, with their
/// byte offsets.
fn words(code: &str) -> Vec<(usize, &str)> {
    let bytes = code.as_bytes();
    let is_identifier = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut found = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if !is_identifier(bytes[index]) || (index > 0 && is_identifier(bytes[index - 1])) {
            index += 1;
            continue;
        }
        let length = bytes[index..]
            .iter()
            .take_while(|byte| is_identifier(**byte))
            .count();
        found.push((index, &code[index..index + length]));
        index += length;
    }
    found
}

/// Every place production code in `source` sets a text size, as `(line
/// number, what was written)`.
///
/// "Sets" means three things: any associated item of `FontSize` - `Px`,
/// whose argument is kept for [`font_size_px`] to read, and every other
/// unit or conversion, which is reported as it stands; a call of one of
/// [`FONT_SIZE_BUILDERS`]; and a `font_size` field given or assigned
/// anything but a `FontSize` expression or a plain name, such as
/// `12.0.into()`. A plain name passes, and so does a `font_size` declared
/// with the type `FontSize`, because that is how a size written as
/// `FontSize::Px` somewhere else - and read there - arrives where it is
/// set: `ui::countdown_card` and the Match screen's `spawn_text` take one.
///
/// Comments, string literals and test-only items are not read, by the same
/// passes the colour guard uses, so a test may set any size it needs.
///
/// # What it does not see
///
/// * **A default.** A `TextFont` that leaves out its size is `bevy_text`'s
///   20px, which is above the floor, so this is not a hole today.
/// * **A size computed from parts**, such as `FontSize::Px(BASE * 0.8)`.
///   That is reported, not passed: [`font_size_px`] reads only a number or
///   a constant declared as one, and anything else is an offender until it
///   is written plainly.
/// * **Where a `FontSize` that is passed in came from**: a name is not
///   followed back. That is safe while every `FontSize` is made by writing
///   one, which this does read, wherever it is written - but a size
///   converted with `.into()` into a `FontSize` parameter would pass.
/// * **Anything that scales text after its size is set** - a `UiScale`, a
///   `Transform`, the web canvas - which is not a size at all.
fn font_size_settings(source: &str) -> Vec<(usize, FontSizeWritten)> {
    let lexemes = lex(source);
    let code = blank_comments_and_strings(source, &lexemes);
    let test_only = test_only_item_ranges(&code);
    let bytes = code.as_bytes();

    let mut found = Vec::new();
    for (index, word) in words(&code) {
        let after = index + word.len();
        let rest = &code[after..];
        let written = if word == "FontSize" && rest.starts_with("::") {
            let length = rest[2..]
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                .count();
            let item = &rest[2..2 + length];
            let open = after + 2 + item.len();
            match (item, bytes.get(open)) {
                ("Px", Some(b'(')) => Some(matching_paren(bytes, open).map_or_else(
                    || FontSizeWritten::Other("FontSize::Px(".into()),
                    |close| FontSizeWritten::Px(code[open + 1..close].trim().into()),
                )),
                _ => Some(FontSizeWritten::Other(format!("FontSize::{item}"))),
            }
        } else if FONT_SIZE_BUILDERS.contains(&word) && rest.starts_with('(') {
            Some(FontSizeWritten::Other(format!("{word}(")))
        } else if word == "font_size" {
            let rest = rest.trim_start();
            let value = if rest.starts_with("::") || rest.starts_with("==") {
                None
            } else {
                rest.strip_prefix(':').or_else(|| rest.strip_prefix('='))
            };
            value
                .map(|value| {
                    let value = value.trim_start();
                    let end = value.find([',', ';', '\n', '}']).unwrap_or(value.len());
                    value[..end].trim()
                })
                .filter(|value| !value.starts_with("FontSize") && !is_plain_name(value))
                .map(|value| FontSizeWritten::Other(format!("font_size: {value}")))
        } else {
            None
        };
        if let Some(written) = written
            && !test_only.iter().any(|item| item.contains(&index))
        {
            found.push((line_number(source, index), written));
        }
    }
    found
}

/// Whether `text` is a name or a field path - `size`, `self.font_size` -
/// rather than an expression: how a `FontSize` made elsewhere arrives.
fn is_plain_name(text: &str) -> bool {
    text.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_')
        && text
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_.".contains(character))
}

/// Every `const NAME: f32 = <number>;` in `source`'s production code, as
/// `(name, value)`. A constant whose value is an expression is left out,
/// so a size named by one is one [`font_size_px`] cannot read.
fn f32_constants(source: &str) -> Vec<(String, f32)> {
    let code = blank_comments_and_strings(source, &lex(source));
    let test_only = test_only_item_ranges(&code);
    let mut found = Vec::new();
    for (index, word) in words(&code) {
        if word != "const" || test_only.iter().any(|item| item.contains(&index)) {
            continue;
        }
        let rest = code[index + word.len()..].trim_start();
        let Some((name, typed)) = rest.split_once(':') else {
            continue;
        };
        let Some((kind, value)) = typed.split_once('=') else {
            continue;
        };
        let value = value.split(';').next().unwrap_or_default();
        if kind.trim() == "f32"
            && let Some(number) = number(value)
        {
            found.push((name.trim().to_string(), number));
        }
    }
    found
}

/// A Rust float or integer literal - `14.0`, `14.`, `14`, `1_4.0_f32` - as
/// a number, or `None` for anything else.
fn number(text: &str) -> Option<f32> {
    let text = text.trim();
    if !text.starts_with(|first: char| first.is_ascii_digit()) {
        return None;
    }
    text.trim_end_matches("f32")
        .trim_end_matches("f64")
        .replace('_', "")
        .parse()
        .ok()
}

/// The size, in logical pixels, that `FontSize::Px(argument)` sets, if this
/// scan can read it: a number written there, or the name of a production
/// constant declared as one. A name declared with two different values
/// anywhere in the crate cannot be read either.
fn font_size_px(argument: &str, constants: &BTreeMap<String, Vec<f32>>) -> Option<f32> {
    number(argument).or_else(|| {
        let values = constants.get(argument)?;
        values
            .iter()
            .all(|value| *value == values[0])
            .then_some(values[0])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_top_level_module_has_a_row_in_the_table() {
        let on_disk: BTreeSet<String> = module_graph().keys().cloned().collect();
        let declared: BTreeSet<String> = ALLOWED_EDGES
            .iter()
            .map(|row| row.module.to_string())
            .collect();

        assert_eq!(
            on_disk, declared,
            "a top-level module was added or removed without updating \
             ALLOWED_EDGES, the checked copy of the graph and the only \
             thing this test compares the source against. Keeping the \
             prose table in docs/ARCHITECTURE.md in step is an editorial \
             obligation that nothing enforces"
        );
    }

    #[test]
    fn module_dependencies_match_the_allowed_edge_table() {
        let graph = module_graph();
        let mut undeclared = Vec::new();
        let mut unused = Vec::new();

        for row in ALLOWED_EDGES {
            let actual = graph.get(row.module).cloned().unwrap_or_default();
            let allowed: BTreeSet<&str> = row.depends_on.iter().copied().collect();

            for (target, files) in &actual {
                if !allowed.contains(target.as_str()) {
                    undeclared.push(format!(
                        "{} -> {target}, named in {:?}",
                        row.module,
                        files.iter().collect::<Vec<_>>()
                    ));
                }
            }
            for target in allowed {
                if !actual.contains_key(target) {
                    unused.push(format!("{} -> {target}", row.module));
                }
            }
        }

        assert!(
            undeclared.is_empty(),
            "these dependencies are not in ALLOWED_EDGES. Either the \
             dependency is wrong and belongs somewhere else, or \
             ALLOWED_EDGES is out of date - do not widen a row without \
             deciding which. Nothing reads docs/ARCHITECTURE.md; if the new \
             edge changes what a module is for, its description there is \
             yours to update by hand:\n  {}",
            undeclared.join("\n  ")
        );
        assert!(
            unused.is_empty(),
            "ALLOWED_EDGES permits dependencies the source no longer has. \
             Remove them - a table that keeps stale permissions stops \
             describing anything. If every edge is listed here, suspect the \
             scanner instead:\n  {}",
            unused.join("\n  ")
        );
    }

    #[test]
    fn every_dependency_cycle_is_declared_with_its_reason() {
        let graph = module_graph();
        let mut found = BTreeSet::new();
        for (module, targets) in &graph {
            for target in targets.keys() {
                let names_back = graph
                    .get(target)
                    .is_some_and(|back| back.contains_key(module));
                if names_back && module < target {
                    found.insert((module.clone(), target.clone()));
                }
            }
        }

        let declared: BTreeSet<(String, String)> = DECLARED_CYCLES
            .iter()
            .map(|cycle| {
                let (first, second) = cycle.between;
                assert!(
                    first < second,
                    "write a declared cycle's modules in alphabetical order: \
                     ({first}, {second})"
                );
                assert!(
                    cycle.reason.len() > 80,
                    "the cycle between {first} and {second} needs a real \
                     reason, not a placeholder"
                );
                (first.to_string(), second.to_string())
            })
            .collect();

        assert_eq!(
            found, declared,
            "a mutual dependency between two top-level modules appeared or \
             disappeared. A new one is not automatically wrong, but it has \
             to be written down with the reason it is allowed - see \
             DECLARED_CYCLES and docs/ARCHITECTURE.md's \"Module ownership \
             and dependency direction\" section"
        );
    }

    #[test]
    fn the_scanner_ignores_comments_test_code_and_literals() {
        let source = r##"
            //! Doc link to [`crate::ui`] that is not a dependency.
            /* block /* nested */ comment mentioning crate::audio */
            use crate::rules::Board;
            use crate::{clock, cue};
            const SLASH: char = '/';
            const QUOTE: char = '"';
            const TEXT: &str = "crate::settings in a string";
            const RAW: &str = r#"crate::input {"; "}"#;
            fn borrow<'a>(value: &'a str) -> &'a str { value }
            #[cfg(test)]
            mod tests { use crate::test_support::advance_test_app; }
            #[cfg(all(feature = "client", test))]
            use crate::gameplay::GameState;
            #[cfg(any(feature = "client", test))]
            use crate::replay::Replay;
            #[cfg(any(target_arch = "wasm32", test))]
            mod browser_storage;
        "##;

        let code = strip_test_only_items(&strip_comments_and_strings(source));
        let named: BTreeSet<String> = crate_paths(&code).into_iter().collect();

        assert_eq!(
            named,
            ["clock", "cue", "replay", "rules"]
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>(),
            "the scanner must see the `use` items and the `crate::{{..}}` \
             group, and must not see doc links, comments, literals, or \
             anything behind a test-only cfg"
        );
        assert!(code.contains("browser_storage"));
    }

    /// A nested group used to be cut at the first inner `}`, which reported
    /// the nested members as top-level modules and lost every sibling after
    /// them. The crate has no such import today, so the loss would have
    /// been silent: an edge in the source that the guard never sees.
    #[test]
    fn the_scanner_reads_every_member_of_a_nested_group() {
        let source = r##"
            use crate::{rules::{Board, Cell}, clock, view::Camera};
            use crate::{
                replay::{Replay, recording::Recorder},
                cue,
            };
            use crate::audio::GameCue;
        "##;
        let named: BTreeSet<String> = crate_paths(source).into_iter().collect();

        assert_eq!(
            named,
            ["audio", "clock", "cue", "replay", "rules", "view"]
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>(),
            "a nested `crate::{{..}}` group contributes the head of the \
             nested path and every sibling after it, and nothing from \
             inside the nested braces"
        );
    }

    /// The failure a source-reading guard has to make impossible: scanning
    /// nothing, and so passing every negative assertion in it.
    #[cfg(feature = "client")]
    #[test]
    #[should_panic(expected = "expected at least")]
    fn a_walk_that_finds_too_few_files_fails_rather_than_passing_vacuously() {
        let _ = sources_in("ui/home", 99);
    }

    #[cfg(feature = "client")]
    #[test]
    #[should_panic(expected = "is not a directory")]
    fn a_walk_pointed_somewhere_that_does_not_exist_fails_immediately() {
        let _ = sources_in("ui/no_such_directory", 1);
    }

    #[test]
    fn a_cfg_holds_under_test_only_when_every_way_of_satisfying_it_does() {
        assert!(cfg_requires_test("test"));
        assert!(cfg_requires_test("all(not(target_arch = ), test)"));
        assert!(cfg_requires_test("any(test, all(test, feature = ))"));
        assert!(!cfg_requires_test("any(feature = , test)"));
        assert!(!cfg_requires_test("not(test)"));
        assert!(!cfg_requires_test("feature = "));
    }

    /// The walk must find the test-only modules as test-only by reading
    /// their declarations, and must not mistake a module that merely also
    /// compiles under `test` for one of them.
    #[test]
    fn test_only_modules_are_found_from_their_declarations() {
        let root = source_root();
        let test_only = test_only_modules(&rust_files_under(&root));
        let names: BTreeSet<String> = test_only
            .iter()
            .filter(|path| path.exists())
            .map(|path| {
                path.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();

        for expected in [
            "architecture.rs",
            "replay/measurement.rs",
            "rules/properties.rs",
        ] {
            assert!(
                names.contains(expected),
                "{expected} is declared behind #[cfg(test)] and must be \
                 excluded from the production graph; found {names:?}"
            );
        }
        for production in ["replay/recording.rs", "platform/browser_storage.rs"] {
            assert!(
                !names.contains(production),
                "{production} compiles outside test builds and must stay in \
                 the production graph"
            );
        }
    }

    /// `CONTRIBUTING.md`'s "UI and accessibility" rule, enforced: every
    /// string literal in the crate's production code is printable ASCII, so
    /// nothing the game assembles from its own literals can reach Bevy's
    /// bundled fallback font as a glyph it lacks and draw as a box. Two
    /// player-facing strings had done exactly that - the round panel's
    /// middle dot and the end-of-match banner's em dash - and nothing
    /// noticed until a player did.
    ///
    /// # Where the line is drawn
    ///
    /// Non-ASCII in this crate's source is not always wrong.
    /// `ui::text_input`'s doc comment explains why an accented letter is
    /// rejected rather than approximated, and its tests, like
    /// `replay::schema`'s, `replay::validation`'s, and the Replay Library's,
    /// type accented letters and control characters precisely to check
    /// that they are refused. So this reads only **string literals**, in
    /// **production code**: [`lex`] never yields a comment as a literal,
    /// and every item behind a `cfg` that only holds under `test`
    /// (`#[cfg(test)] mod tests { .. }` above all, but also the
    /// `#[cfg(all(feature = "client", test))]` items in `lib.rs` and
    /// `replay`) is cut out by
    /// [`test_only_item_ranges`], the pass the module-dependency guard
    /// uses. Escapes are decoded, so `"\u{2014}"` fails as a bare em dash
    /// does, and `"\t"` fails as a tab.
    ///
    /// It is deliberately wider than the rule it enforces. It cannot tell a
    /// player-facing literal from a log message or a file name, so it holds
    /// all of them to printable ASCII, byte strings included. Nothing in the
    /// crate needs otherwise; a literal that legitimately does belongs
    /// behind a test `cfg` if it is a fixture, or built at runtime if it is
    /// not.
    ///
    /// # What it does not see
    ///
    /// * **`char` literals.** `ui::text_input` maps typographic quotes and
    ///   dashes to their ASCII stand-ins with `'\u{2018}'`-style patterns
    ///   in production code, so holding `char` literals to the rule would
    ///   need an allowlist. A non-ASCII `char` pushed into a `String`
    ///   therefore passes.
    /// * **Text that is not a literal**: `char::from_u32`, arithmetic on
    ///   bytes, and anything read at runtime from a file, a Replay label, a
    ///   settings file, or the keyboard. Typed text is `ui::text_input`'s
    ///   to refuse, and it has tests of its own.
    /// * **Whether a literal is player-facing at all.** A non-ASCII log
    ///   message is reported like a banner; see above.
    ///
    /// A failure names the file, the literal's first line, and the first
    /// offending character as a code point.
    #[test]
    fn production_string_literals_are_printable_ascii() {
        let root = source_root();
        let mut scanned = 0usize;
        let mut offenders = Vec::new();
        for path in production_source_files() {
            let source = read(&path);
            let relative = path.strip_prefix(&root).unwrap_or(&path).display();
            for (line, literal) in production_string_literals(&source) {
                scanned += 1;
                if let Some(character) = first_undrawable(literal) {
                    offenders.push(format!(
                        "src/{relative}:{line}: U+{:04X} in {}",
                        character as u32,
                        excerpt(literal)
                    ));
                }
            }
        }

        assert!(
            scanned >= MINIMUM_PRODUCTION_STRING_LITERALS,
            "found only {scanned} string literals in production code, fewer \
             than the {MINIMUM_PRODUCTION_STRING_LITERALS} the crate is known \
             to have: suspect the lexer or the cfg stripping before the code"
        );
        assert!(
            offenders.is_empty(),
            "these string literals in production code are not printable \
             ASCII, which Bevy's bundled fallback font draws as a \
             missing-glyph box (CONTRIBUTING.md, \"UI and accessibility\"). \
             Use words and ordinary punctuation; a test fixture belongs \
             behind a test cfg, and a doc comment is free to say anything:\n  {}",
            offenders.join("\n  ")
        );
    }

    /// `CONTRIBUTING.md`'s contrast floor is checked over a table of
    /// pairings in `src/contrast.rs`, and that table is held complete by
    /// requiring every colour constant in `presentation.rs` to appear in it.
    /// That only means something if `presentation.rs` is where colours come
    /// from: a colour written inline in a screen would be drawn, and checked
    /// by nothing. So no production code outside that file may construct a
    /// colour or derive one - see [`colour_constructions`] for exactly what
    /// counts.
    ///
    /// The repair for a failure is to name the colour in `presentation.rs`
    /// and use the name; the contrast guard will then ask where it is drawn.
    ///
    /// This runs in both build configurations, since it reads source text,
    /// which is what lets a `--no-default-features` run hold `ui` code it
    /// never compiles.
    #[test]
    fn colours_are_constructed_only_in_presentation() {
        let root = source_root();
        let colour_file = root.join(COLOUR_FILE);
        let mut offenders = Vec::new();
        let mut declared = 0usize;
        for path in production_source_files() {
            let constructions = colour_constructions(&read(&path));
            if path == colour_file {
                declared = constructions.len();
                continue;
            }
            let relative = path.strip_prefix(&root).unwrap_or(&path).display();
            for (line, written) in constructions {
                offenders.push(format!("src/{relative}:{line}: {written}"));
            }
        }

        assert!(
            declared >= MINIMUM_COLOUR_CONSTRUCTIONS,
            "found only {declared} colour constructions in src/{COLOUR_FILE}, fewer than \
             the {MINIMUM_COLOUR_CONSTRUCTIONS} it is known to hold: suspect the scan \
             before concluding the rest of the crate is clean"
        );
        assert!(
            offenders.is_empty(),
            "these lines make a colour outside src/{COLOUR_FILE}. Name it there instead and \
             use the name, so that the contrast guard in src/contrast.rs can ask where it \
             is drawn (CONTRIBUTING.md, \"UI and accessibility\"):\n  {}",
            offenders.join("\n  ")
        );
    }

    /// Each thing the colour guard must report and each thing it must let
    /// through, in one sample.
    #[test]
    fn the_colour_guard_reads_constructions_and_nothing_else() {
        let sample = r#"
/// A doc comment may show `Color::srgb(1.0, 0.0, 0.0)`.
fn spawn(commands: &mut Commands) {
    // So may a comment: Color::WHITE.
    let hidden = Color::NONE;
    let border = BorderColor::all(ORANGE);
    let text = TextColor(TEXT_PRIMARY);
    let label = "Color::srgb in a string";
    let inline = Color::srgb(0.1, 0.2, 0.3);
    let named = Color::WHITE;
    let faded = PANEL_BACKGROUND.with_alpha(0.5);
    let other = Srgba::new(0.1, 0.2, 0.3, 1.0);
    let lighter = ORANGE.lighter(0.1);
    let unrelated = remix(with_alpha);
    let mut changed = ORANGE;
    changed.set_alpha(0.5);
    let by_path = Alpha::with_alpha(&ORANGE, 0.5);
    let wrapped = TextColor::WHITE;
    let border = BorderColor::DEFAULT;
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_may_compare_against_a_literal() {
        assert_eq!(ORANGE, Color::srgb(0.95, 0.56, 0.24));
    }
}
"#;
        let found: Vec<(usize, String)> = colour_constructions(sample);
        let written: Vec<&str> = found.iter().map(|(_, written)| written.as_str()).collect();
        assert_eq!(
            written,
            [
                "Color::srgb",
                "Color::WHITE",
                ".with_alpha(",
                "Srgba",
                ".lighter(",
                ".set_alpha(",
                ".with_alpha(",
                "TextColor::WHITE",
            ]
        );
        assert_eq!(found[0].0, 9, "the line of the inline constructor");
    }

    /// `CONTRIBUTING.md` puts a floor under every text size in the game -
    /// [`TEXT_FLOOR_PX`] - and this is what holds it. Until 2026-10-07 the
    /// smallest text was an 11px validation error, and the audit that found
    /// it was a grep for `FontSize::Px` of a number, which missed a 13px
    /// size set through a named constant. So this reads sizes the way the
    /// code sets them - a number where it is written, or a constant declared
    /// as one anywhere in the crate, see [`font_size_px`] - and a size it
    /// cannot read fails rather than passing.
    ///
    /// The repair for a size below the floor is to raise it and then look at
    /// the screen again, because larger text can wrap a line that used to
    /// fit; `docs/TESTING.md` carries the probe. A size this cannot read
    /// should be written as a number, or as a constant whose value is one.
    ///
    /// What it reads is the size text is set at, so a scale applied on top
    /// is invisible to it. The game applies one, `bevy_ui`'s `UiScale`, in
    /// one place - `frame_main_camera` in `app::visuals` - and only to draw
    /// a window smaller than the minimum, 1040x600, scaled down to fit: it
    /// is 1 in every window at least the minimum, and never above 1. That
    /// window is the one place text is drawn below the floor, by design.
    ///
    /// Like the other guards that read source, this runs in both build
    /// configurations.
    #[test]
    fn no_text_is_set_below_the_floor() {
        let root = source_root();
        let files: Vec<(PathBuf, String)> = production_source_files()
            .into_iter()
            .map(|path| {
                let source = read(&path);
                (path, source)
            })
            .collect();
        let mut constants: BTreeMap<String, Vec<f32>> = BTreeMap::new();
        for (_, source) in &files {
            for (name, value) in f32_constants(source) {
                constants.entry(name).or_default().push(value);
            }
        }

        let mut checked = 0usize;
        let mut offenders = Vec::new();
        for (path, source) in &files {
            let relative = path.strip_prefix(&root).unwrap_or(path).display();
            for (line, written) in font_size_settings(source) {
                let place = format!("src/{relative}:{line}");
                match written {
                    FontSizeWritten::Px(argument) => match font_size_px(&argument, &constants) {
                        Some(px) if px >= TEXT_FLOOR_PX => checked += 1,
                        Some(px) => {
                            offenders.push(format!("{place}: FontSize::Px({argument}) is {px}px"));
                        }
                        None => offenders.push(format!(
                            "{place}: FontSize::Px({argument}) is not a number or a constant \
                             declared as one, so its size cannot be checked"
                        )),
                    },
                    FontSizeWritten::Other(written) => offenders.push(format!(
                        "{place}: {written} sets a size some way other than FontSize::Px"
                    )),
                }
            }
        }

        assert!(
            checked >= MINIMUM_FONT_SIZES,
            "found only {checked} text sizes in production code, fewer than the \
             {MINIMUM_FONT_SIZES} the crate is known to set: suspect the scan before \
             concluding nothing is below the floor"
        );
        assert!(
            offenders.is_empty(),
            "these lines set text below the {TEXT_FLOOR_PX}px floor, or in a way this guard \
             cannot read (CONTRIBUTING.md, \"UI and accessibility\"). Raise the size and \
             measure the screen again - larger text can wrap a line that fitted:\n  {}",
            offenders.join("\n  ")
        );
    }

    /// Each way of setting a size the floor guard must read or report, and
    /// each thing it must let through, in one sample.
    #[test]
    fn the_text_floor_guard_reads_sizes_and_nothing_else() {
        let sample = r#"
/// A doc comment may show `FontSize::Px(9.0)`.
const LABEL_SIZE: f32 = 16.0;
pub(super) const HINT_SIZE: f32 = 1_2.0;
const DOUBLED: f32 = LABEL_SIZE * 2.0;
struct Card {
    font_size: FontSize,
}
fn spawn(commands: &mut Commands, size: FontSize) {
    // So may a comment: FontSize::Px(9.0).
    let label = "FontSize::Px(9.0) in a string";
    let literal = TextFont { font_size: FontSize::Px(14.0), ..default() };
    let named = TextFont { font_size: FontSize::Px(LABEL_SIZE), ..default() };
    let carried = TextFont { font_size: size, ..default() };
    let computed = FontSize::Px(LABEL_SIZE * 0.8);
    let other_unit = FontSize::Vh(2.0);
    let converted = FontSize::from(12.0);
    let built = TextFont::from_font_size(12.0);
    let into = TextFont { font_size: 12.0.into(), ..default() };
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_may_set_any_size() {
        let tiny = TextFont { font_size: FontSize::Px(1.0), ..default() };
    }
}
"#;
        let found = font_size_settings(sample);
        let px = |argument: &str| FontSizeWritten::Px(argument.into());
        let other = |written: &str| FontSizeWritten::Other(written.into());
        let written: Vec<&FontSizeWritten> = found.iter().map(|(_, written)| written).collect();
        assert_eq!(
            written,
            [
                &px("14.0"),
                &px("LABEL_SIZE"),
                &px("LABEL_SIZE * 0.8"),
                &other("FontSize::Vh"),
                &other("FontSize::from"),
                &other("from_font_size("),
                &other("font_size: 12.0.into()"),
            ]
        );
        assert_eq!(found[0].0, 12, "the line of the literal size");

        let constants: BTreeMap<String, Vec<f32>> = f32_constants(sample)
            .into_iter()
            .map(|(name, value)| (name, vec![value]))
            .collect();
        assert_eq!(
            constants.keys().collect::<Vec<_>>(),
            ["HINT_SIZE", "LABEL_SIZE"],
            "a constant whose value is an expression is not read"
        );
        assert_eq!(font_size_px("14.0", &constants), Some(14.0));
        assert_eq!(font_size_px("LABEL_SIZE", &constants), Some(16.0));
        assert_eq!(font_size_px("HINT_SIZE", &constants), Some(12.0));
        assert_eq!(font_size_px("LABEL_SIZE * 0.8", &constants), None);
        assert_eq!(font_size_px("size", &constants), None);
        let ambiguous = BTreeMap::from([("SIZE".to_string(), vec![14.0, 12.0])]);
        assert_eq!(
            font_size_px("SIZE", &ambiguous),
            None,
            "a name declared with two values is not read"
        );
    }

    /// Each kind of literal the guard must read or must skip, in one
    /// sample: doc and block comments, an ordinary string with a line
    /// break, an escaped quote and a continuation, a raw string, a `char`
    /// literal, and fixtures behind both spellings of a test-only `cfg`,
    /// beside the four ways a production literal can fail. The sample is a
    /// raw string, so the escapes in it reach the lexer as the text they
    /// would be in a source file - and this file's own literals stay
    /// printable.
    #[test]
    fn the_ascii_guard_reads_production_string_literals_and_nothing_else() {
        let source = r##"
            //! Doc comment naming an \u{e9}, a \u{2014} and a \u{b7} for a reader.
            /* block comment: \u{b7} */
            const PLAIN: &str = "printable, with a\nline break and a \"quote\"";
            const RAW: &str = r#"raw "quoted" text"#;
            const CONTINUED: &str = "one \
                                     two";
            const DOT: &str = "Round \u{b7}";
            const ESCAPED: &str = "dash \u{2014}";
            const HEX: &[u8] = b"delete \x7f";
            const TAB: &str = "tabbed\t";
            const DASH: char = '\u{2014}';
            #[cfg(test)]
            mod tests {
                const FIXTURE: &str = "Caf\u{e9} { not a brace }";
                fn typed() { insert("\u{e9}"); }
            }
            #[cfg(all(feature = "client", test))]
            const CLIENT_FIXTURE: &str = "\u{fc}";
            #[cfg(any(feature = "client", test))]
            const SHARED: &str = "printable";
        "##;
        let literals = production_string_literals(source);
        let offenders: Vec<(usize, char)> = literals
            .iter()
            .filter_map(|(line, literal)| first_undrawable(literal).map(|found| (*line, found)))
            .collect();

        assert_eq!(literals.len(), 9, "{literals:?}");
        assert_eq!(
            offenders,
            vec![(8, '\u{b7}'), (9, '\u{2014}'), (10, '\u{7f}'), (11, '\t')],
            "a middle dot, an escaped em dash, a hex-escaped delete, and an \
             escaped tab are the four failures; the comments, the char \
             literal, and everything behind a test-only cfg are not seen"
        );
    }

    /// The exclusions, exercised on real code rather than a sample:
    /// `ui::text_input` names an accented letter in a doc comment and types
    /// one in a test, and the guard must see neither. If that file stops
    /// containing non-ASCII, this test no longer proves anything and should
    /// be pointed at one that does.
    #[test]
    fn the_ascii_guard_skips_the_doc_comment_and_the_fixture_in_text_input() {
        let source = source_file("ui/text_input.rs");
        assert!(
            !source.is_ascii(),
            "ui/text_input.rs no longer contains non-ASCII text, so it no \
             longer exercises the guard's exclusions"
        );
        let seen: Vec<_> = production_string_literals(&source)
            .into_iter()
            .filter(|(_, literal)| first_undrawable(literal).is_some())
            .collect();
        assert!(
            seen.is_empty(),
            "the guard reported a literal in a doc comment or a test: {seen:?}"
        );
    }

    #[test]
    fn a_literal_is_read_as_the_font_would_draw_it() {
        assert_eq!(first_undrawable(r#""plain text""#), None);
        assert_eq!(first_undrawable(r#""two\nlines""#), None);
        assert_eq!(first_undrawable(r#""quote \" and slash \\""#), None);
        assert_eq!(first_undrawable("\"one \\\n    two\""), None);
        assert_eq!(first_undrawable(r#"r"raw \u{2014} is text""#), None);
        assert_eq!(first_undrawable(r#""\u{2014}""#), Some('\u{2014}'));
        assert_eq!(first_undrawable(r#""\u{00_b7}""#), Some('\u{b7}'));
        assert_eq!(first_undrawable(r#""\x7f""#), Some('\u{7f}'));
        assert_eq!(first_undrawable(r#""\t""#), Some('\t'));
        assert_eq!(first_undrawable(r#""\r""#), Some('\r'));
        assert_eq!(first_undrawable(r#""\0""#), Some('\0'));
        assert_eq!(
            excerpt("\"a help page\nwith more lines\""),
            "\"a help page..."
        );
    }
}
