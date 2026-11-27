#!/usr/bin/env python3
"""Keep the build machine's paths out of a release build, and check that they are out.

A compiled Rust program records the path of every source file a panic could
name. Built the ordinary way, those are absolute paths on the machine that
built it: each dependency's files under Cargo's home, the standard library's
under the toolchain when its source is installed there, and any generated file
under the build directory. A published build would publish them, and with them
the builder's user name and folder layout.

`run` starts a build command with `--remap-path-prefix` flags added to its
environment, so that rustc records a short stand-in for each of those
locations. `check` reads what is to be published - files, folders and the
archives made from them - and fails if any of it holds one of this machine's
locations, a path in a shape the flags would have removed, the shape of a home
folder, or the name of the account that built it:

    python3 scripts/machine_paths.py run cargo build --locked --release
    python3 scripts/machine_paths.py check go-together-v1.2.3-linux.tar.gz

Both ask this machine where its locations are and what the account running
them is called, so neither names anybody. `scripts/build_native.py` and
`scripts/build-web.sh` do both for the two release builds, `path_search.py`
does the reading, and `docs/DISTRIBUTION.md` says what a release promises.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass, field
import functools
import getpass
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

import path_search


REPOSITORY = Path(__file__).resolve().parents[1]
# The same folder as it was typed to reach this file, which a symbolic link on
# the way makes a different spelling.
REPOSITORY_AS_RUN = Path(os.path.abspath(__file__)).parents[1]

# Cargo's separator between the flags in CARGO_ENCODED_RUSTFLAGS. Unlike the
# spaces of RUSTFLAGS it cannot occur in a path, so a home folder with a space
# in its name survives.
FLAG_SEPARATOR = "\x1f"

# Folders at the top of the checkout that hold build output, not sources.
NOT_SOURCE = ("target", "dist")

# A package that `cargo tree` prints with the folder it is built from.
TREE_LINE = re.compile(r"(?P<name>\S+) v(?P<version>\S+) \((?P<folder>(?:/|[A-Za-z]:).*?)\)")


@dataclass
class Location:
    """One place on this machine that a build could record.

    `stand_in` is what rustc is told to record instead; None where nothing is
    expected to record the place, which `check` still looks for.
    """

    name: str
    path: Path
    stand_in: str | None = None


@dataclass
class Survey:
    locations: list = field(default_factory=list)
    # The account's name as it is looked for in files, and every name an
    # archive member's owner is compared with.
    names: list = field(default_factory=list)
    owners: list = field(default_factory=list)
    # Where Cargo said it builds, for `build_native.py` to hold it to.
    build_directories: list = field(default_factory=list)
    # What could not be found out, each as an item of "Not looked for: ...".
    unknown: list = field(default_factory=list)


def ask(command: list) -> tuple:
    """Run a tool from the checkout: what it prints, or None and why not."""
    try:
        return (
            subprocess.run(
                command, cwd=REPOSITORY, check=True, capture_output=True, text=True
            ).stdout,
            "",
        )
    except OSError:
        return None, f"`{command[0]}` could not be run"
    except subprocess.CalledProcessError as error:
        said = (error.stderr or "").strip().splitlines()
        # Its own words, without this machine's spelling of the checkout.
        reason = said[0].replace(str(REPOSITORY), ".") if said else f"status {error.returncode}"
        return None, f"`{' '.join(command[:2])}` failed ({reason})"


def cargo_metadata(manifest: Path | None = None) -> tuple:
    """Ask Cargo about a manifest's workspace, without resolving dependencies.

    Cargo answers with every configuration file and environment variable
    applied, which is the only reliable way to learn where it builds.
    """
    command = ["cargo", "metadata", "--format-version", "1", "--no-deps"]
    if manifest is not None:
        command += ["--manifest-path", str(manifest)]
    answer, why = ask(command)
    try:
        return (json.loads(answer), "") if answer else (None, why)
    except ValueError:
        return None, "`cargo metadata` answered with something that is not JSON"


def under(path: Path, folder: Path) -> bool:
    try:
        path.resolve().relative_to(folder.resolve())
    except ValueError:
        return False
    return True


def outside_dependencies(metadata: dict, result: Survey) -> None:
    """Add the path dependencies kept outside the checkout, following them onward.

    Their sources are recorded by absolute path, as a registry crate's are.
    One that Cargo will not describe is still a location; what it depends on
    in turn is then unknown, and said to be. A package that the lock file
    builds from a folder and that none of this finds, as one replaced through
    `[patch]` is not, is looked for in `cargo tree`, which knows it once a
    build has fetched the crates; one still not found is named as not looked
    for.
    """
    found = {package["name"] for package in metadata["packages"]}
    asked: set = set()
    waiting = [metadata]
    while waiting:
        for package in waiting.pop()["packages"]:
            for dependency in package["dependencies"]:
                folder = dependency.get("path")
                if not folder or folder in asked:
                    continue
                asked.add(folder)
                found.add(dependency["name"])
                stand_in = f"/cargo/deps/{dependency['name']}"
                onward, why = cargo_metadata(Path(folder) / "Cargo.toml")
                if onward is None:
                    result.unknown.append(
                        f"what the dependency `{dependency['name']}` depends on in turn, since {why}"
                    )
                else:
                    waiting.append(onward)
                    for candidate in onward["packages"]:
                        found.add(candidate["name"])
                        if Path(candidate["manifest_path"]).parent == Path(folder):
                            stand_in = f"/cargo/deps/{candidate['name']}-{candidate['version']}"
                if not under(Path(folder), REPOSITORY):
                    result.locations.append(
                        Location("a dependency outside the checkout", Path(folder), stand_in)
                    )

    try:
        lock = (Path(metadata["workspace_root"]) / "Cargo.lock").read_text(encoding="utf-8")
    except OSError:
        return
    missing = []
    for package in lock.split("[[package]]")[1:]:
        fields = dict(line.split(" = ", 1) for line in package.splitlines() if " = " in line)
        name = fields.get("name", "").strip('"')
        if "source" not in fields and name not in found:
            missing.append(name)
    if missing:
        tree = ["cargo", "tree", "--offline", "--locked", "--all-features"]
        listed = ask(tree + ["--prefix", "none", "--format", "{p}"])[0] or ""
        for line in listed.splitlines():
            match = TREE_LINE.match(line)
            if match and match["name"] in missing:
                missing.remove(match["name"])
                if not under(Path(match["folder"]), REPOSITORY):
                    stand_in = f"/cargo/deps/{match['name']}-{match['version']}"
                    result.locations.append(
                        Location("a dependency outside the checkout", Path(match["folder"]), stand_in)
                    )
    for name in missing:
        result.unknown.append(
            f"the folder of `{name}`, which Cargo.lock builds from a folder that neither a "
            "dependency nor `cargo tree` names (one replaced through `[patch]`, perhaps)"
        )


@functools.lru_cache(maxsize=None)
def survey() -> Survey:
    """Ask this machine where a build's sources are and what its account is called.

    The stand-ins are modelled on those of Cargo's own `trim-paths` profile
    setting, which is not stable in the toolchain this project pins and can
    replace these flags when it is.
    """
    result = Survey(
        [
            Location("the checkout", REPOSITORY, "."),
            Location("the checkout", REPOSITORY_AS_RUN, "."),
        ]
    )

    metadata, why = cargo_metadata()
    if metadata is None:
        target = os.environ.get("CARGO_TARGET_DIR") or os.environ.get("CARGO_BUILD_TARGET_DIR")
        folders = [str(REPOSITORY / (target or "target"))]
        result.unknown.append(
            f"where Cargo builds, since {why} (`{target or 'target'}` in the checkout is "
            "assumed), and any dependency kept outside the checkout"
        )
    else:
        # Cargo's own spelling of the checkout, which is the one it builds by.
        result.locations.append(Location("the checkout", Path(metadata["workspace_root"]), "."))
        outside_dependencies(metadata, result)
        folders = sorted(
            {
                metadata["target_directory"],
                metadata.get("build_directory") or metadata["target_directory"],
            }
        )
    result.build_directories = folders
    for folder in folders:
        result.locations.append(Location("the build directory", Path(folder), "/cargo/build-dir"))

    try:
        home = Path.home()
    except RuntimeError:
        home = None
        result.unknown.append("the home folder, which this system does not give")
    if home is not None:
        result.locations.append(Location("the home folder", home))

    for name, variable, default, stand_in in (
        ("Cargo's home", "CARGO_HOME", ".cargo", "/cargo"),
        ("rustup's home", "RUSTUP_HOME", ".rustup", None),
    ):
        if os.environ.get(variable):
            result.locations.append(Location(name, Path(os.environ[variable]), stand_in))
        elif home is not None:
            result.locations.append(Location(name, home / default, stand_in))

    rustc = os.environ.get("RUSTC", "rustc")
    sysroot, why = ask([rustc, "--print", "sysroot"])
    if sysroot:
        # Not the sysroot itself, which may be as plain as `/usr`. The
        # standard library's own build records its sources as
        # `/rustc/<commit>/library/...`; an installed copy of them is recorded
        # by its real path unless it is given exactly that name here.
        rustlib = Path(sysroot.strip()) / "lib" / "rustlib"
        described = ask([rustc, "-vV"])[0] or ""
        version = dict(line.split(": ", 1) for line in described.splitlines() if ": " in line)
        commit = version.get("commit-hash", "unknown")
        if commit == "unknown":
            commit = version.get("release", commit)
        result.locations.append(Location("the Rust toolchain", rustlib))
        result.locations.append(
            Location("the standard library's source", rustlib / "src" / "rust", f"/rustc/{commit}")
        )
    else:
        result.unknown.append(f"the Rust toolchain, since {why}")

    # The name three ways, since they can differ: the home folder's, the one
    # the environment gives, and the one the system's account database has
    # for this process, which is the one `tar` writes as an owner.
    names = [home.name] if home is not None else []
    try:
        names.append(getpass.getuser())
    except Exception:  # No account entry and no variable: what is raised varies.
        pass
    try:
        import pwd

        names.append(pwd.getpwuid(os.getuid()).pw_name)
    except (ImportError, KeyError, AttributeError):  # Windows has no such module.
        pass
    for name in names:
        if not name or name.casefold() in (known.casefold() for known in result.owners):
            continue
        result.owners.append(name)
        if len(name) > 1:
            result.names.append(name)
        else:
            result.unknown.append(
                "an account name of one letter in a file, where it is in every file "
                "(an archive's owners are still compared with it)"
            )
    if not result.owners:
        result.unknown.append("this account's name, which the system does not give")

    return result


def spellings(path: Path) -> list:
    """Return the ways a build may have written `path`, or none for a root.

    Cargo passes some locations on as the environment gives them and others as
    the system resolves them, and the two differ when a folder on the way is a
    symbolic link. A filesystem root is left out: it is a prefix of every
    path, so it says nothing about this machine.
    """
    absolute = Path(os.path.abspath(path))
    found = {str(absolute), str(absolute.resolve())}
    return sorted(spelling for spelling in found if Path(spelling).parent != Path(spelling))


def searchable(spelling: str) -> bool:
    """Say whether a location is long enough to look for in a built file.

    A compiled program stores its text end to end, so nothing marks where a
    path begins, and a folder directly under the root - a checkout at `/src`
    in a container, or `/root` as a home folder - is also the middle of other
    paths, such as `.../src/lib.rs`. A drive letter is mark enough.
    """
    path = Path(spelling)
    return len(path.parts) > (1 if path.drive else 2)


def own_paths() -> list:
    """List the checkout's files that have two folders above them, relative to it.

    A build records its own sources by these paths. One of them can hold the
    text of a location by chance - `src/ui/home/mod.rs` holds `/home/mo` - and
    is not that location. A path with fewer than two separators cannot.
    """
    found = []
    for folder, folders, files in os.walk(REPOSITORY):
        at_top = Path(folder) == REPOSITORY
        folders[:] = sorted(
            name
            for name in folders
            if not name.startswith(".") and not (at_top and name in NOT_SOURCE)
        )
        for name in files:
            relative = (Path(folder) / name).relative_to(REPOSITORY).as_posix()
            if relative.count("/") >= 2:
                found.append(relative)
    return found


def wanted() -> path_search.Wanted:
    """Turn the survey into what `path_search` looks for."""
    machine = survey()
    result = path_search.Wanted(
        names=list(machine.names),
        own_paths=own_paths(),
        skipped=list(machine.unknown),
        owners=list(machine.owners),
    )
    usable: dict = {}
    for location in machine.locations:
        for spelling in spellings(location.path):
            if searchable(spelling):
                kept = usable.setdefault(location.name, [])
                if spelling not in kept:
                    kept.append(spelling)
            else:
                skipped = f"{location.name}, too short a path to tell from part of another"
                if skipped not in result.skipped:
                    result.skipped.append(skipped)
    result.locations = list(usable.items())
    return result


def remap_flags() -> list:
    """Return the rustc flags that replace each location with its stand-in.

    When several prefixes match a path rustc applies the last one given, so a
    location inside another has to come after it: they are given shortest
    first, by how many folders deep each is.
    """
    pairs = []
    for location in survey().locations:
        if location.stand_in is not None:
            for spelling in spellings(location.path):
                if (spelling, location.stand_in) not in pairs:
                    pairs.append((spelling, location.stand_in))
    pairs.sort(key=lambda pair: len(Path(pair[0]).parts))
    return [f"--remap-path-prefix={spelling}={stand_in}" for spelling, stand_in in pairs]


def remapping_environment(environment=os.environ) -> dict:
    """Return `environment` with the remapping flags added to its rustc flags.

    The flags already there are kept, such as the `-D warnings` CI sets in
    RUSTFLAGS. Cargo reads CARGO_ENCODED_RUSTFLAGS before RUSTFLAGS and reads
    only one of them, so both are folded into the first. Rustflags from
    Cargo's configuration are not used, whether a file gives them or
    CARGO_BUILD_RUSTFLAGS or CARGO_TARGET_<TRIPLE>_RUSTFLAGS does: Cargo reads
    those only when the two variables above are both absent.
    """
    result = dict(environment)
    if "CARGO_ENCODED_RUSTFLAGS" in result:
        flags = [flag for flag in result["CARGO_ENCODED_RUSTFLAGS"].split(FLAG_SEPARATOR) if flag]
    else:
        flags = result.get("RUSTFLAGS", "").split()
    result.pop("RUSTFLAGS", None)
    result["CARGO_ENCODED_RUSTFLAGS"] = FLAG_SEPARATOR.join(flags + remap_flags())
    return result


def announce(environment=os.environ) -> None:
    """Say, as a build starts, what was done to its flags and what is not used.

    A maintainer whose own configuration changes how a program is linked gets
    a different program from these builds, and should hear it from the build.
    """
    ignored = sorted(
        name
        for name in environment
        if name == "CARGO_BUILD_RUSTFLAGS"
        or (name.startswith("CARGO_TARGET_") and name.endswith("_RUSTFLAGS"))
    )
    print(
        f"machine_paths: {len(remap_flags())} path-remapping flags join the rustc flags in the "
        "environment; rustflags from Cargo's configuration are not used"
        + (f" ({', '.join(ignored)} is set and ignored)" if ignored else "")
        + ".",
        file=sys.stderr,
    )


def examine(paths, names_fail: bool = False, name_is_a_word: bool = False) -> path_search.Report:
    """Read what is at or under `paths` for this machine's paths and this account's name.

    The check first reads the cases it plants itself, and one that comes out
    wrong is reported in place of everything else. `names_fail` says that what
    is read is to be published: the account's name and the shape of a home
    folder are then errors, where the build scripts, which also run for people
    whose name is a word, note them. `name_is_a_word` notes the name even so.
    """
    looked_for = wanted()
    failures = path_search.self_test(looked_for)
    if failures:
        report = path_search.Report()
        report.problems = [f"the check cannot be trusted: {failure}" for failure in failures]
        return report
    search = path_search.Search(looked_for, names_fail, name_is_a_word)
    return search.read([Path(path) for path in paths])


def tell(report: path_search.Report, verdict: str) -> int:
    """Print a report's notes and errors, or `verdict` and what it rests on."""
    for line in report.notes:
        print(f"note: {line}", file=sys.stderr)
    if report.errors:
        for line in report.errors:
            print(f"error: {line}", file=sys.stderr)
        return 1
    print(f"{verdict} {report.summary}")
    return 0


def prove(distribution: Path) -> list:
    """Run the real commands on what they must refuse, and say which did not refuse.

    The cases `path_search` plants are judged inside one process, by the
    errors a report holds. What they cannot see is the step after: that each
    script turns those errors into a failing exit status. So this runs the
    scripts themselves, as CI and a maintainer do, on a copy of a clean web
    distribution with one path planted in it, on a file that names this
    account, and on an archive this account owns. `verify_web_build.py` calls
    it after every verification that passes.
    """
    scripts = Path(__file__).resolve().parent
    planted = b"/opt/planted/registry/src/index.crates.io-0123456789abcdef/planted-1.0.0/src/lib.rs"
    failures = []

    def passes(script: str, *arguments, naming: str = "") -> bool:
        result = subprocess.run(
            [sys.executable, str(scripts / script), *arguments], capture_output=True, text=True
        )
        return result.returncode == 0 or naming not in result.stderr

    with tempfile.TemporaryDirectory() as folder:
        copy = Path(folder) / "dist"
        shutil.copytree(distribution, copy)
        (copy / "planted.bin").write_bytes(planted)
        if passes("verify_web_build.py", "--without-proof", str(copy), naming="planted.bin"):
            failures.append("verify_web_build.py passes a distribution with a path planted in it")

        small = Path(folder) / "small"
        small.mkdir()
        (small / "clean.txt").write_bytes(b"clean\n")
        word = ("check", "--name-is-a-word", str(small))
        if not passes("machine_paths.py", *word):
            failures.append("machine_paths.py check fails a folder with nothing in it to find")
        (small / "planted.bin").write_bytes(planted)
        if passes("machine_paths.py", *word, naming="planted.bin"):
            failures.append("machine_paths.py check passes a file with a path planted in it")
        (small / "planted.bin").unlink()

        machine = survey()
        for name in machine.names[:1]:
            (small / "named.txt").write_text(f"built by {name.upper()}\n", encoding="utf-8")
            if passes("machine_paths.py", "check", str(small), naming="named.txt"):
                failures.append("machine_paths.py check passes a file that names this account")
            if not passes("machine_paths.py", *word):
                failures.append(
                    "machine_paths.py check --name-is-a-word fails a file that names this account"
                )
            (small / "named.txt").unlink()
        for name in machine.owners[:1]:
            owned = path_search.tar_of((path_search.tar_member(owner=name), b"clean"))
            (small / "owned.tar").write_bytes(owned)
            if passes("machine_paths.py", *word, naming="owned.tar"):
                failures.append(
                    "machine_paths.py check --name-is-a-word passes an archive this account owns"
                )
    return failures


def run(command: list) -> int:
    announce()
    try:
        return subprocess.call(command, cwd=REPOSITORY, env=remapping_environment())
    except OSError as error:
        print(f"error: cannot run {command[0]}: {error}", file=sys.stderr)
        return 1


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Keep this machine's paths out of a release build, or check that they are out."
    )
    commands = parser.add_subparsers(dest="command", required=True)
    run_parser = commands.add_parser(
        "run",
        help="run a build command from the repository root with the path-remapping rustc flags",
    )
    run_parser.add_argument("build_command", nargs=argparse.REMAINDER, metavar="COMMAND ...")
    check_parser = commands.add_parser(
        "check",
        help="fail if a file, folder or archive holds a path of this machine or the name of "
        "the account running this",
    )
    check_parser.add_argument(
        "--name-is-a-word",
        action="store_true",
        help="note this account's name where a file holds it, and do not fail on it: for "
        "someone whose account name is also an ordinary word",
    )
    check_parser.add_argument("paths", nargs="+", type=Path, metavar="PATH")
    arguments = parser.parse_args()

    if arguments.command == "run":
        if not arguments.build_command:
            run_parser.error("a command to run is required")
        return run(arguments.build_command)
    report = examine(
        arguments.paths, names_fail=True, name_is_a_word=arguments.name_is_a_word
    )
    return tell(report, "Nothing of this machine found.")


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(130)
