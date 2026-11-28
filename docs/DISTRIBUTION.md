# Distribution

Go Together does not currently publish release artifacts. No binaries or
archives have been uploaded, signed, or notarized, and no release checksums have
been published. This guide records a future maintainer workflow; none of the
commands below imply that a release happened.

## Versioning and release inputs

Use a `vMAJOR.MINOR.PATCH` Git tag that matches the package version in
`Cargo.toml`. Build from a clean, reviewed tag with the locked dependency graph,
record the Rust and Trunk versions, and keep the resulting checksums with the
release notes.

The Cargo release profile uses thin LTO, one code-generation unit, and stripped
symbols. Those size-oriented choices trade longer builds and less useful crash
diagnostics for smaller downloads. Re-measure that tradeoff before changing the
profile rather than treating the settings as universally optimal.

## Paths of the build machine

A release build must not carry the paths of the machine that built it. A
compiled Rust program records the path of every source file a panic could
name, and built the ordinary way those paths are absolute: each dependency's
files under Cargo's home, the standard library's under the toolchain when its
source is installed there (the `rust-src` component, which rust-analyzer
needs), and any generated file under the build directory. Measured on
2026-11-26 on a macOS host, a plain `cargo build --locked --release` left the
builder's home folder in the executable 1,687 times and a plain Trunk build
left it in the `.wasm` 1,456 times. `strip = "symbols"` does not remove them:
they are text the program can print, not symbols. Uploading either file would
publish the builder's user name and folder layout, and not only to someone who
opens the file: Bevy's logger names the source file of every line it writes,
so a web build made that way prints the home folder in each player's browser
console as it starts.

So `scripts/build_native.py` and `scripts/build-web.sh` build with rustc's
`--remap-path-prefix`, which records a stand-in for each location:

| On the build machine | Recorded as |
| --- | --- |
| Cargo's home: `CARGO_HOME`, or `.cargo` in the home folder | `/cargo` |
| The standard library's source, `lib/rustlib/src/rust` in the toolchain | `/rustc/<commit>`, the name the precompiled standard library already uses |
| The build directory, wherever Cargo's configuration puts it (`target/` unless told otherwise) | `/cargo/build-dir` |
| A dependency kept in a folder outside the checkout | `/cargo/deps/<name>-<version>` |
| The checkout | `.` |

Built that way on the same day, neither file held the home folder at all. A
panic still names the crate and the file it came from. Two of the locations
the executable records, before and after:

```text
<home>/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_ecs-0.20.0/src/system/system_param.rs
/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_ecs-0.20.0/src/system/system_param.rs

<home>/.rustup/toolchains/<toolchain>/lib/rustlib/src/rust/library/std/src/sync/once.rs
/rustc/88d9e12ae178fab0fb5cc050a94da85685d449ea/library/std/src/sync/once.rs
```

The game's own files are recorded relative to the checkout (`src/...`) either
way, so a panic in the game's code reads the same in every build.

A Linux build measured the same way the next day, in an arm64 container with
Rust 1.98.1 whose Cargo home is `/usr/local/cargo`, held 1,631 paths of that
machine in a plain executable and none in a scripted one, and none in a web
distribution built there as CI builds it. No Windows build has been made with
these scripts yet, and the parts of the check that exist for Windows - either
slash, either case, a drive letter - have not run there either. The check
plants its own cases on every run (see
[Checking what is published](#checking-what-is-published)), Windows paths
among them, so the first build there tests both: the build by the check, and
the check by its cases.

The stand-ins are modelled on those of Cargo's `trim-paths` profile setting,
which would do all of this from `Cargo.toml`, with no script. It is not stable
in Rust 1.98, where Cargo refuses a manifest that uses it. They differ in one
place: `trim-paths` records a registry crate as
`/cargo/registry/<hash>/<crate>-<version>/...`, with a short name for the
registry that only Cargo can compute, where these flags replace Cargo's home
as a whole and so keep `registry/src/index.crates.io-<hash>/` in the path.
Once the setting is stable, set it in `[profile.release]` and remove the flags
from `scripts/machine_paths.py`; the check below stays.

The flags reach rustc through the environment, added to whatever `RUSTFLAGS`
already holds, so CI's `-D warnings` still applies to the web build. Cargo
takes rustflags from its configuration only when the environment gives none,
so a maintainer's own `build.rustflags` or `target.<triple>.rustflags` do not
apply to these two builds, whether a configuration file sets them or
`CARGO_BUILD_RUSTFLAGS` or `CARGO_TARGET_<TRIPLE>_RUSTFLAGS` does. If yours
change how a program is linked - a static C runtime on Windows, say - these
builds differ from your own. Each script prints a line saying so as it starts,
and names those two variables when they are set.

Cargo gives flags from the environment to the target's crates only, once it
has a `--target`, which the web build always has and a native build may be
given. Build scripts and procedural macros are then compiled without them, so
a macro that writes its own source path into the code it generates is recorded
as it is. Nothing in the game's dependencies does that today, and the check
reports it if one starts to.

Nothing else changes. Development builds, tests and the other required checks
set no such flag, and a failing test still reports the real path it panicked
at. Cargo keeps the two kinds of release build apart under `target/`, so
neither rebuilds the other, and each costs its own disk space: about 1.8 GB
for a native release build and 1.2 GB for a web one on that host. (A web
build's further 0.2 GB of build scripts and macros is shared, since the flags
do not reach them.)

What a build leaves in place is not kept apart. `target/release/go-together`
is whichever kind was built last, so a `cargo run --release` after the script
puts the paths back, and `trunk serve` replaces all of `dist/` with a
development build: measured on 2026-11-27, its `.wasm` held the home folder
13,928 times. So the check that counts is the one on what is published.

### Checking what is published

```sh
python3 scripts/machine_paths.py check go-together-vMAJOR.MINOR.PATCH-*
```

Run it on the files that will be downloaded, on the machine that built them.
It reads every file at or under each path it is given, through symbolic links,
whose own targets it reads as well. It reads into zip archives, one behind the
program that unpacks it included; into tar archives, with or without the
`ustar` mark and joined end to end; and through gzip, bzip2, xz, the older
lzma format and zstd, where the Python running it has the module for the
format (zstd came with Python 3.14). In a tar it also reads the owner and
group recorded for each member.

It refuses what it knows it cannot read: a folder with no file in it, a link
to nothing, an archive that will not open or lies more than four deep in
others, a compressed file whose module this Python lacks, and the formats it
recognises and has no reader for. Those are 7z and RAR, also behind an
unpacking program; cpio, cabinet, xar and pkg; an installer database; RPM and
Debian packages; squashfs, an AppImage and a disk image; LZ4, lzip and `.Z`.
For one of these, check the folder it was made from immediately before it is
made, and record that the file itself was not read. Any other file is read as
the bytes it is, an installer made by some other tool among them, so what
such a file has compressed inside it is not seen. The last line of a result
says how many archives were opened, which is how to tell.

It looks for four things:

- **This machine's locations**: the home folder, Cargo's home, rustup's home,
  the toolchain, the checkout, the build directory and any dependency kept
  outside the checkout, as `cargo metadata`, `rustc` and the environment give
  them when it runs. Each is looked for wherever it occurs, as UTF-8, UTF-16
  and UTF-32, and on Windows with either slash and in either case. One thing
  is passed over: a location's text inside one of the game's own relative
  paths, or inside a source path under one of the stand-ins, where it stops
  part-way through a name. `src/ui/home/mod.rs` is in every build, and is not
  the home folder of a contributor called `mo`. A location directly under the
  filesystem root, such as a checkout at `/src` in a container, is too short
  to tell from part of another path, and is not looked for.
- **A path in a shape the flags would have removed**, whatever machine
  recorded it: a registry or git dependency's source that is not under
  `/cargo`, the standard library's source under `lib/rustlib/src/rust`, a
  rustup toolchain folder. This is what finds a build made on another
  machine, or with `CARGO_HOME` set differently from now. It has one blind
  spot: a Cargo home that is itself a folder called `cargo`, as
  `/usr/local/cargo` is in the official Rust image, records
  `.../cargo/registry/src/...`, which nothing tells from the stand-in.
  Checked where it was built, that home is one of the locations above and is
  found.
- **A path in the shape of a home folder**: `/Users/<name>/`, `/home/<name>/`
  or `\Users\<name>\`. It covers what nothing above does: a file made on
  another machine by another tool, as an asset may be. One that begins inside
  one of the game's own paths, or inside a source path under a stand-in, is
  that path and is passed over: the game has a folder called `src/ui/home/`.
- **The builder's user name**: the name of the account the check runs as, as
  the system gives it, in lower case, in upper case and capitalised, which for
  a name of plain letters is any case. It is what a path would have given
  away, and it travels without one: an ordinary `tar czf` records it as the
  owner of every member (see [Native packages](#native-packages) for a
  command that does not).

The first two fail wherever they are found. The last two fail this command and
are noted by the two build scripts, which every contributor and CI run: a
user name may be an ordinary word (`runner`, on a GitHub runner, is in a
winit file name), and a build script reads text that nobody here wrote. Two
things follow for a maintainer. If your user name is an ordinary word,
`--name-is-a-word` makes this command note it too; an archive member owned by
your account fails regardless, since an owner is never a word. And a signed
file carries its signer's name, so where your user name is part of that name
this command fails on every signed file, and `--name-is-a-word` is the way
through. There is no such switch for the shape of a home folder: one that is
not a leak is text some dependency carries, and the way to pass it is to add
that exact text, and where it comes from, to the check itself.

Before it reads anything, the check plants cases of its own in a temporary
folder. Most belong to an invented machine, so that they come out the same
whatever this machine is called and wherever it keeps things: its locations
and names in each form the check claims to find, and beside everything the
check passes over, the nearest thing it must not. The rest are this machine's
own locations and names, plainly. Each case is judged by whether the command
would fail on it - this command, this command with `--name-is-a-word`, and a
build script - and if one comes out wrong the check stops with "the check
cannot be trusted". A check that had quietly stopped failing would otherwise
pass every build it was shown, and CI only ever shows it builds that are meant
to be clean.

Its last line says what it read and looked for, and names anything it could
not look for. It knows this machine as it is when it runs, so run it where the
build was made and with the environment the build had. It finds text, not
meaning: a path written another way - with JSON's escaped slashes,
percent-encoded in a URL, or compressed inside a file format it does not open
- is not found as a path, though the builder's user name in it usually is.

Both build scripts run the check, on what they have just built:
`scripts/build_native.py` on the executable, and `scripts/build-web.sh` on
every file in `dist/`, through `scripts/verify_web_build.py`. That script
then runs the scripts themselves on what they must refuse: itself on a copy
of the distribution with a path planted in it, and this command on a file
that names this account and on an archive the account owns. So CI's `web` job
fails if the web build loses the property, if the check stops failing on one
of its planted cases, or if a script stops failing when the check does. No CI
job builds a native release, and packaging adds files and archives of its
own, which is why the [release testing checklist](#release-testing-checklist)
runs the check on the files that are published.

The check is more than a test of the flags. A dependency can keep a path as
data instead of as a source location - `env!("CARGO_MANIFEST_DIR")` stored in
a string is the usual way - and no rustc flag rewrites that. None of the
game's dependencies does today; the check is what would say so if one began
to.

### If the check reports a path

The text after "such as" is what was found, and usually names the crate.

- **Thousands, under Cargo's home or as registry paths with no stand-in.**
  The file was not built by the scripts: a plain `cargo build --release` or
  `trunk serve` has replaced it. Build again with `scripts/build_native.py`
  or `scripts/build-web.sh`.
- **A few `.rs` paths in a build that is otherwise clean.** The flags did not
  reach one compilation. With a `--target` that is a build script's or a
  macro's own path, written into the code it generates, and only changing or
  replacing the dependency removes it.
- **A folder, not a `.rs` file.** A dependency keeps a path as data:
  `env!("CARGO_MANIFEST_DIR")` or `env!("OUT_DIR")` in a string. No flag
  rewrites data, so the dependency has to change or go.
- **Under the home folder and nothing more exact.** The build read something
  from a place the scripts do not know of. Find what the excerpt names, and
  move it into the checkout or leave it out.
- **"Cargo wrote to ..., which is not where it said it would build".** A
  `--target-dir` or `--config` argument moved the build after the flags were
  made. Give the build directory in `CARGO_TARGET_DIR` or a configuration
  file, where Cargo can be asked about it first.
- **The builder's user name, or the shape of a home folder.** In a tar,
  archive again with the command under [Native packages](#native-packages).
  In a file, read the excerpt, and see the two switches described above.
- **A file the check refuses.** Check the folder it was made from, immediately
  before it is made, and record that the file itself was not read.
- **"the check cannot be trusted".** The check came out wrong on one of its
  own cases, on this system or after a change to it. Nothing it says about a
  build means anything until that is fixed.

If what is reported is plainly not a path of yours - other text that happens
to hold a short folder name - that is a fault in the check, and worth an
issue. Read the line before posting it: it names a file by its full path
when the file is outside the folder the check was run from. The check already
passes over the game's own source paths and the stand-ins for this reason,
and a new case belongs beside them.

## Native packages

Build native releases on each target operating system rather than assuming that
an untested cross-build is distributable:

```sh
python3 scripts/build_native.py
```

The script runs `cargo build --locked --release` with the flags described under
[Paths of the build machine](#paths-of-the-build-machine), then fails if the
executable holds one of this machine's paths. Arguments are passed on to Cargo,
so `python3 scripts/build_native.py --target x86_64-apple-darwin` builds another
target the same way. Give a build directory in `CARGO_TARGET_DIR` or a
configuration file, not as `--target-dir` or `--config` on the command line:
the flags are made from where Cargo says it will build before it sees those,
and the script fails after the build if Cargo wrote anywhere else. It needs
Python 3.9 or newer and nothing outside the standard library; where Windows
installs Python as `py` rather than `python3`, run
`py scripts\build_native.py`. A plain `cargo build --locked --release` still
builds the game, but not an executable to publish.

Expected release targets are:

| Platform | Typical Rust target | Executable |
| --- | --- | --- |
| macOS Apple silicon | `aarch64-apple-darwin` | `go-together` |
| macOS Intel | `x86_64-apple-darwin` | `go-together` |
| Windows 64-bit | `x86_64-pc-windows-msvc` | `go-together.exe` |
| Linux 64-bit | `x86_64-unknown-linux-gnu` | `go-together` |

Every native package must bundle the complete `assets/` directory **next to the
executable**:

```text
Go Together/
├── go-together[.exe]
├── LICENSE
└── assets/
    └── audio/
        ├── README.md
        └── *.wav
```

Installed releases resolve this adjacent `assets/` first. A current-working-
directory fallback exists for development runs, but a package must not depend on
the directory from which a player launches it.

Copy `assets/` into the package folder instead of linking it, and archive the
folder so that the archive itself names nobody. A zip made with `zip -r`
records no owner. A tar records the name and group of whoever made it, on
every member, unless it is told not to:

```sh
# macOS (bsdtar). --no-mac-metadata also leaves out the `._` members that
# would carry each file's extended attributes.
tar --no-mac-metadata --uname '' --gname '' --uid 0 --gid 0 \
  -czf go-together-vMAJOR.MINOR.PATCH-macos.tar.gz "Go Together"

# Linux (GNU tar)
tar --owner=0 --group=0 --numeric-owner \
  -czf go-together-vMAJOR.MINOR.PATCH-linux.tar.gz "Go Together"
```

Then check the archive, not only the folder it was made from:
`python3 scripts/machine_paths.py check go-together-vMAJOR.MINOR.PATCH-*`
reads into it and fails on an owner's name as it does on a path. See
[Checking what is published](#checking-what-is-published).

Platform finishing remains future work:

- **macOS:** package an `.app`, sign its nested code and bundle with a Developer
  ID certificate, submit it to Apple notarization, staple the ticket, and test
  Gatekeeper on a clean machine. Asset resolution looks for `assets/` only in
  the directory holding the executable (`src/platform/native.rs`), so an `.app`
  must ship `assets/` inside `Contents/MacOS/` beside the binary rather than
  under the conventional `Contents/Resources/`, unless that resolution is first
  taught about bundle layouts. A universal build requires both macOS targets
  and a tested universal executable.
- **Windows:** sign the executable and final installer/archive with an
  Authenticode certificate and timestamp the signature. Test SmartScreen and
  launch from a standard user account.
- **Linux:** preserve the executable bit (`chmod 755 go-together`), state the
  supported distribution/runtime assumptions, and test the archive after
  extraction. Packaging as AppImage, Flatpak, or distribution packages would be
  separate future work.

Do not describe a native package as signed or notarized until the final,
downloadable bytes have been verified.

## HTML5 build for itch.io

Install the pinned builder and create the verified distribution. The script
also needs `python3` on `PATH` - it starts Trunk through
`scripts/machine_paths.py`, then runs `scripts/verify_web_build.py` over
`dist/` - and exits early when `python3` is missing:

```sh
cargo install trunk --locked --version 0.21.14
scripts/build-web.sh
```

The script builds only `dist/`; it does not zip, upload, publish, or access an
account. It builds with the same flags as the native script and its
verification reads every file in `dist/` for a path of this machine; see
[Paths of the build machine](#paths-of-the-build-machine). The archive contents
must begin with `index.html`, not a wrapping `dist/` directory:

```text
index.html
go-together-<hash>.js
go-together-<hash>_bg.wasm
LICENSE
assets/
└── audio/
    ├── README.md
    └── *.wav
web/
├── shell.js
└── styles.css
```

All runtime paths are relative so this layout works at itch.io's generated game
URL. It has no CDN dependency, analytics, PWA manifest, or service worker. For a
manual archive after verification, a maintainer may run:

```sh
(cd dist && zip -r ../go-together-vMAJOR.MINOR.PATCH-html5.zip .)
python3 scripts/machine_paths.py check go-together-vMAJOR.MINOR.PATCH-html5.zip
```

The second command reads the archive that will be uploaded. It matters
because `dist/` is also where `trunk serve` writes: a development session
between the build and the archive replaces every file in it.

Create an itch.io **HTML** project, mark the archive as playable in the browser,
and give it a viewport of at least 1056x650. The canvas fills the frame less
the page's padding and the status line under the canvas, 16 px across and
about 49 px down - measured in Chrome on 2026-11-28, where a 1056x949 frame
gave the canvas the game's opening 1040x900 - and the game lays itself out at
the canvas's size from 1040x600 up, so a frame of any shape at least that
large is used whole. A smaller frame still works, with the canvas laid out at
1040x600 and drawn scaled down to fit. Enable itch.io's fullscreen control and
test both the embedded and fullscreen layouts. Do not require a top-level navigation or popup: the game is designed to
remain inside the itch iframe. Keyboard play is supported; browser gamepads are
best-effort and require the browser/device to expose a standard mapping.

The web build stores settings and at most the newest three completed Replays in
browser storage. Test quota failure and blocked/private storage before
publishing. Settings v1-v6 and Replay v1-v6 are compatibility inputs on both
web and native builds. Packaging must not bypass normal validation of immutable
Replay v5+ rule snapshots, integer-second bounds, timeout frames, or end
reasons. Replay v6 additionally omits an event's board on disk when it is
unchanged from the previous event (see
[Replay size](ARCHITECTURE.md#replay-size)); it is resolved back transparently
on load, so the in-memory frame always carries a complete board and packaging
has no influence on either side of that transform.

### Starting in a browser

`web/shell.js` downloads and compiles the WebAssembly module while the page
loads, then shows "Click or press a key to start." and waits for a pointer press
on the canvas, or a key press while the canvas has focus. That gesture starts
the app: the handler awaits wasm-bindgen's asynchronous initializer with the
compiled module, which instantiates it and runs `main`. Bevy's audio output is
built during that start-up, and cpal creates and resumes its `AudioContext`
there.

The instantiation must stay asynchronous. Chromium refuses to instantiate a
module larger than 8 MB synchronously on the main thread ("WebAssembly.Instance
is disallowed on the main thread, if the buffer size is larger than 8MB"), and
the release `.wasm` is about three times that. Until 2026-10-27 the shell called
wasm-bindgen's synchronous `initSync`, and desktop Chrome 156 then showed "The
game could not start" after a click and after a key alike; Firefox 158 and
Safari 26.6 accepted it.

Audio still starts from that first gesture, because the `AudioContext` is
created while the gesture's activation is live. Measured on an M1 Mac on
2026-10-27, the context was created 0.13-0.38 s after the gesture (Firefox
0.13-0.38 s, Safari 0.17-0.20 s, Chrome 0.21-0.32 s). In all three it was
running, and sound reached its output once Home's Play was pressed, whether
the first gesture was a click or a key. What each browser requires differs, and
Safari is the strict one:

| Browser | Lets an `AudioContext` start once the page has had | Context created 6 s after the gesture |
| --- | --- | --- |
| Chrome 156 | any user activation (sticky) | runs |
| Firefox 158 | any user activation (sticky) | runs |
| Safari 26.6 | a gesture within about the last 5 s (transient) | stays suspended |

In Safari a context created 4.5 s after the gesture ran, and one created at
5.5 s stayed suspended for good: cpal resumes it once, when it creates it, and
nothing resumes it later. So keep the path from the gesture to the app's start
short. The module is compiled before the prompt appears so that only the
instantiation is left; do not put a download or other slow work between the
gesture and the instantiation.

Before publishing, start the game by a click and by a key in Chrome, Firefox and
Safari, both at the top level and inside itch.io's frame, and check that a
button's cue is heard after each. A change here can work in one browser and
fail in another. The measurements above were taken at the top level only.

## Build profiles

Measured on this project's own dependency tree (a native `aarch64-apple-darwin`
host, Rust 1.98, Bevy 0.20.0, on 2026-10-27); a different platform or Bevy
version would give different absolute numbers, but the relative comparisons
should hold. The upgrade from Bevy 0.19.1 to 0.20.0 grew both builds: the
same code measured the same day on 0.19.1 gave ~48.7 MiB native and
~24.6 MiB of `.wasm`.

The flags that keep the build machine's paths out (see
[Paths of the build machine](#paths-of-the-build-machine)) move none of these
figures. Measured on the same host on 2026-11-26, with and without them: the
native executable was 16 KB smaller with them (55,749,424 bytes against
55,765,936, ~53.2 MiB either way), the `.wasm` 24 KB smaller (27,334,182
against 27,358,523, ~26.1 MiB either way), and the JS glue the same 124,287
bytes. A clean build took the same time: 3 min 5 s for the native executable
and 1 min 45 s for the web distribution, each within 2 s of the build without
the flags.

**Native** (`[profile.release]`: `codegen-units = 1`, `lto = "thin"`,
`opt-level = 3`, `strip = "symbols"`): a release build of the `go-together`
binary is currently ~53.2 MiB. `opt-level = "z"` was measured on native too and
rejected: this project is not CPU-bound in a way that would make the
runtime-speed tradeoff worthwhile, and a local desktop install has no
comparable download-size pressure to a browser player - unlike the WASM case
below, so the existing settings (already balancing runtime responsiveness,
size, and build time reasonably) are kept as-is. `panic = "abort"` was not
adopted: it would trade away Rust's default unwind-based panic diagnostics
(useful during active development and for a player's crash report) for a
build-size reduction not large enough, relative to Bevy's own footprint, to
justify losing that.

**WASM** (`[profile.wasm-release]`, used only for `scripts/build-web.sh`'s
`trunk build --release --cargo-profile wasm-release` - never for local `trunk
serve` development, which stays on Cargo's ordinary fast `dev` profile):
inherits the native release profile but overrides `opt-level = "z"`. Measured
result: the built `.wasm` drops from ~47.7 MiB (`opt-level = 3`, i.e. sharing
the native profile) to ~26.1 MiB - a ~45% reduction - with the JS glue size
unchanged (~121 KB either way, since that dimension isn't affected by Rust
optimization level at all). For a browser player, download and WASM
instantiation time scale with this size directly, so the tradeoff (slightly
slower generated code, in exchange for a dramatically smaller download) was
judged clearly worth it for the one build where players actually experience
download size as a real wait. `opt-level = "s"` was not separately measured
in detail once "z" showed this large a win; a future maintainer curious about
the middle ground can measure it the same way: edit `opt-level` in
`[profile.wasm-release]`, re-run `scripts/build-web.sh`, and compare the
`dist/*_bg.wasm` size, which is the artifact the figures above describe. A
bare `cargo build --release --target wasm32-unknown-unknown` is not
comparable - it selects `[profile.release]` rather than
`[profile.wasm-release]`, and leaves the larger pre-`wasm-bindgen` `.wasm`
under `target/` rather than the file Trunk emits.

**`wasm-opt` remains deliberately not enabled.** Trunk can invoke it
automatically, but doing so means Trunk downloads a `wasm-opt` binary release
from GitHub with no pinned version or verified checksum in this project's
configuration today - see `docs/SECURITY.md`'s note that "the checked-in
build does not enable an automatically downloaded WebAssembly optimizer;
adding a native build tool requires an immutable source and verified
checksum." That supply-chain bar, not a measured lack of benefit, is why it
stays off; a maintainer who wants to revisit this would need to pin an exact
`wasm-opt` version/checksum first, not just flip a Trunk config flag. The
setting that keeps it off is `data-wasm-opt="0"` on the `<link data-trunk
rel="rust">` element in `index.html`; `Trunk.toml` carries no `wasm_opt`
entry.

Fat LTO (`lto = "fat"` instead of `"thin"`) and higher/lower `codegen-units`
were not separately re-measured for the figures above: the `thin`/`1`
combination is set explicitly in `[profile.release]` rather than left at
Cargo's defaults, and the figures were measured to choose an `opt-level`.
Revisiting it is reasonable future work.

## Optional Butler uploads

Butler can be added later as a deliberate, authenticated maintainer step. Keep
credentials outside the repository and replace every placeholder:

```sh
butler push dist <itch-account>/<itch-project>:html5 --userversion MAJOR.MINOR.PATCH
butler push "Go Together Windows" <itch-account>/<itch-project>:windows --userversion MAJOR.MINOR.PATCH
butler push "Go Together Linux" <itch-account>/<itch-project>:linux --userversion MAJOR.MINOR.PATCH
butler push "Go Together.app" <itch-account>/<itch-project>:mac --userversion MAJOR.MINOR.PATCH
```

These are examples only. The repository automation does not invoke Butler and
no upload has been performed by this project. Butler uploads a folder as it
is, with no archive of the maintainer's making in between, so the folder is
the last thing there is to check: run
`python3 scripts/machine_paths.py check <folder>` on each one immediately
before pushing it.

## Checksums

Generate checksums only after signing and creating the final archives, because
either operation changes the bytes:

```sh
shasum -a 256 go-together-vMAJOR.MINOR.PATCH-*
```

On Windows PowerShell:

```powershell
Get-FileHash .\go-together-vMAJOR.MINOR.PATCH-* -Algorithm SHA256
```

Publish the exact filenames, sizes, and SHA-256 values together. Checksums detect
accidental or malicious replacement; they do not replace code signing or a
trusted download channel.

## Release testing checklist

- [ ] The tag and Cargo package version agree, and the checkout is clean.
- [ ] Both CI jobs pass on the tagged commit: `native` (documentation links,
      formatting, Clippy with warnings denied and tests for both the default
      and `--no-default-features` configurations, private-item rustdoc, `cargo
      build`, and the generated-audio check) and `web` (WASM Clippy, WASM
      private-item rustdoc, and `scripts/build-web.sh`). CI runs on a push to
      `main` and on a pull request; where the commit has no run, start one by
      hand from the Actions tab's *Run workflow* button.
- [ ] The advisory scan's latest findings have been read: dispatch
      `.github/workflows/audit.yml` or run `cargo audit` locally. It gates
      nothing, so a green CI run says nothing about it; see
      [SECURITY.md](SECURITY.md#dependencies-and-automation).
- [ ] Each native executable was built by `scripts/build_native.py`, and
      `dist/` by `scripts/build-web.sh`.
- [ ] `python3 scripts/machine_paths.py check` passes on every file that will
      be downloaded - each archive, the ones the checksums are made from - and
      on each folder pushed with Butler, run on the machine that built it,
      after the last change to it. Its last line counts the archives it
      opened and names nothing it could not look for, or each thing named has
      been judged. For a file it refuses or does not open, such as a disk
      image or an installer, the folder the file was made from passed
      immediately before it was made, and the release notes say that the file
      itself was not read (see
      [Checking what is published](#checking-what-is-published)).
- [ ] Each native package starts with `assets/` beside the executable.
- [ ] Home, Settings, Match, Replay Library, and Replay Viewer work on each OS.
- [ ] Keyboard and two-controller behavior are tested where supported.
- [ ] Native settings and Replays survive restart; corrupt data fails safely.
- [ ] `dist/index.html` starts from an extracted HTML5 archive with no network CDN.
- [ ] Embedded and fullscreen web layouts fill the frame, and a frame smaller
      than 1056x650 shows the whole game scaled down.
- [ ] Web keyboard input, supported standard-mapped gamepads, storage limits, and
      interaction-gated audio are tested in Chrome, Firefox, and Safari (see
      [Starting in a browser](#starting-in-a-browser)).
- [ ] Final native artifacts have the intended permissions and signatures.
- [ ] SHA-256 checksums were made from the final downloadable bytes.
- [ ] A clean machine/browser can download, verify, extract, start, and remove the release.
