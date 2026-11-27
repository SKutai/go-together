#!/usr/bin/env python3
"""Read built files, and archives of them, for a machine's paths and an account's name.

`machine_paths.py` knows where this machine keeps things and what the account
building on it is called; this module knows how to look. Given a `Wanted`, a
`Search` reads every file at or under a path, through symbolic links and into
zip and tar archives, and reports:

- one of the machine's locations, wherever it occurs and whatever surrounds it;
- a path in a shape that only a build made without the remapping flags leaves,
  on whatever machine it was made;
- a path in the shape of a home folder, and the account's name, which is what
  such a path gives away and which an archive can carry with no path at all,
  as the owner of each file in it.

The first two always fail. The last two fail where what is published is
checked and are noted where a build is, since a build script also runs for
people whose name is a word and over text that nobody here wrote.

It refuses what it knows it cannot read: a folder with no file in it, a link
to nothing, an archive it cannot open. A file in a format it does not know is
read as the bytes it is. And before it reads anything it plants cases of its
own (`self_test`): an invented machine's paths and names in every form it
claims to find, and beside each thing it passes over the nearest thing it must
not. Each case is judged by whether the check would fail, because a check that
had quietly stopped failing would pass every build it was shown.
"""

from __future__ import annotations

from dataclasses import dataclass, field
import importlib
import io
import os
from pathlib import Path
import re
import tarfile
import tempfile
import zipfile


def optional(name: str):
    """Import a module that a Python may have been built without."""
    try:
        return importlib.import_module(name)
    except ImportError:
        return None


# How the same text may be held: UTF-8 as Rust writes it, UTF-16 as Windows
# resources and C's wide strings there do, UTF-32 as wide strings elsewhere do.
ENCODINGS = ("utf-8", "utf-16-le", "utf-16-be", "utf-32-le", "utf-32-be")

# What a build made without the flags leaves, wherever it was made: a name for
# reports, the text that starts a match (for a fast first pass), the pattern
# tried there, and whether it is the shape of a home folder, which fails only
# where what is published is checked. A registry or git path is fine after the
# `/cargo` stand-in and nowhere else, which leaves one blind spot: a Cargo home
# under `/` whose own folder is called `cargo` looks the same, as the official
# Rust image's `/usr/local/cargo` does. The standard library's source is fine
# as `/rustc/<commit>/library`, which does not contain `rustlib`.
SHAPES = (
    (
        "a registry source path with no stand-in",
        (b"registry",),
        re.compile(rb"(?<!/cargo[/\\])registry[/\\]src[/\\][0-9A-Za-z._-]+-[0-9a-f]{16}[/\\]"),
        False,
    ),
    (
        "a git dependency's path with no stand-in",
        (b"checkouts",),
        re.compile(
            rb"(?<=git[/\\])(?<!/cargo[/\\]git[/\\])"
            rb"checkouts[/\\][0-9A-Za-z._-]+-[0-9a-f]{16}[/\\]"
        ),
        False,
    ),
    (
        "the standard library's source by its real path",
        (b"rustlib",),
        re.compile(rb"rustlib[/\\]src[/\\]rust[/\\]library[/\\]"),
        False,
    ),
    (
        "a rustup toolchain folder",
        (b".rustup",),
        re.compile(rb"(?<=[/\\])\.rustup[/\\]toolchains[/\\]"),
        False,
    ),
    (
        "a path in the shape of a home folder",
        (b"/Users/", b"/home/", b"\\Users\\"),
        re.compile(rb"(?:/Users/|/home/|\\Users\\)[0-9A-Za-z._ \x80-\xff-]{1,40}[/\\]"),
        True,
    ),
)

# A source path under one of the stand-ins is the build's own text, not the
# machine's: what the flags of `machine_paths.py` record, and what the
# precompiled standard library records for itself and its dependencies.
# Nothing separates it from what is stored next, so it is taken to end at its
# first `.rs`, and to be no longer than twice the longest such path a real
# build has been seen to hold.
STAND_IN = re.compile(
    rb"(?:/cargo[/\\](?:registry|git|build-dir|deps)|/rustc/[0-9a-f]{40}|/rust/deps)"
    rb"[/\\][\x21-\x7e]{0,200}?\.rs"
)
STAND_IN_STARTS = (b"/cargo", b"/rustc/", b"/rust/deps")
STAND_IN_REACH = 260

# Formats that hide their contents and that nothing here can open. A file in
# one of them is refused, where reading its bytes would find nothing and call
# that clean. The first two are also found behind the program that unpacks
# them, which is how a self-extracting archive is made.
OPAQUE = (
    ("a 7z archive", lambda data: b"7z\xbc\xaf\x27\x1c" in data),
    ("a RAR archive", lambda data: b"Rar!\x1a\x07" in data),
    ("an LZ4 file", lambda data: data.startswith(b"\x04\x22\x4d\x18")),
    ("an lzip file", lambda data: data.startswith(b"LZIP")),
    ("a compress (.Z) file", lambda data: data.startswith(b"\x1f\x9d")),
    ("a cabinet file", lambda data: data.startswith(b"MSCF")),
    ("a cpio archive", lambda data: data.startswith((b"0707", b"\xc7\x71", b"\x71\xc7"))),
    ("a xar or pkg archive", lambda data: data.startswith(b"xar!")),
    ("an installer database or other OLE file", lambda data: data.startswith(b"\xd0\xcf\x11\xe0")),
    ("an RPM package", lambda data: data.startswith(b"\xed\xab\xee\xdb")),
    ("a Debian package", lambda data: data.startswith(b"!<arch>\ndebian-binary")),
    ("a squashfs image", lambda data: data.startswith(b"hsqs")),
    ("an AppImage", lambda data: data.startswith(b"\x7fELF") and data[8:11] == b"AI\x02"),
    ("a disk image", lambda data: data[-512:-508] == b"koly"),
)

# One compressed stream, read as what it holds: how it begins, and the module
# that opens it. The older lzma format has no mark of its own: it begins with
# the settings every tool writes, a dictionary size that is a power of two,
# and a length that is unknown or believable. zstd is in the standard library
# from Python 3.14, and may begin with a frame to be skipped, as `pzstd`
# writes it.
LZMA_SIZES = {bytes(pair) for bit in range(8) for pair in ((1 << bit, 0), (0, 1 << bit))}
PACKED = (
    ("a gzip file", lambda data: data.startswith(b"\x1f\x8b"), "gzip"),
    ("a bzip2 file", lambda data: data.startswith(b"BZh") and data[4:10] == b"1AY&SY", "bz2"),
    ("an xz file", lambda data: data.startswith(b"\xfd7zXZ\x00"), "lzma"),
    (
        "an lzma file",
        lambda data: data.startswith(b"\x5d\x00\x00")
        and data[3:5] in LZMA_SIZES
        and (data[5:13] == b"\xff" * 8 or data[11:13] == b"\x00\x00"),
        "lzma",
    ),
    (
        "a zstd file",
        lambda data: data.startswith(b"\x28\xb5\x2f\xfd")
        or (data[1:4] == b"\x2a\x4d\x18" and 0x50 <= data[0] <= 0x5F),
        "compression.zstd",
    ),
)
MODULES = {module: optional(module) for _, _, module in PACKED}

REFUSED = (
    "check the folder it was made from, immediately before it is made, and record that "
    "the file itself was not read"
)
MAXIMUM_DEPTH = 4
EXAMPLE_LENGTH = 120


@dataclass
class Wanted:
    """What to look for, as `machine_paths.wanted` gathers it."""

    # A name for reports and the ways the location may be written.
    locations: list = field(default_factory=list)
    # The name of the account, as the machine gives it; more than one when its
    # ways of giving it disagree.
    names: list = field(default_factory=list)
    # The checkout's own files, as paths relative to it. A build records its
    # own sources this way, and `src/ui/home/mod.rs` is not somebody's
    # `/home/mo`.
    own_paths: list = field(default_factory=list)
    # What could not be looked for, and why, for the last line of a report.
    skipped: list = field(default_factory=list)
    # On Windows a path may be written in either case.
    fold_case: bool = os.name == "nt"
    # Names that an archive member's owner is compared with, besides `names`:
    # one too short to look for in a file can still be compared exactly.
    owners: list = field(default_factory=list)
    # The modules this Python has for opening a compressed file, by name.
    openers: dict = field(default_factory=lambda: MODULES)


@dataclass
class Finding:
    where: str
    kind: str
    text: str
    error: bool


@dataclass
class Report:
    findings: list = field(default_factory=list)
    problems: list = field(default_factory=list)
    files: int = 0
    opened: int = 0
    members: int = 0
    looked_for: str = ""
    skipped: list = field(default_factory=list)

    @property
    def errors(self) -> list:
        lines = list(self.problems)
        failed = [item for item in self.findings if item.error]
        lines.extend(f"{item.where}: {item.text}" for item in failed)
        if failed:
            lines.append(
                'what is published must hold none of these: see "Paths of the build machine" '
                "in docs/DISTRIBUTION.md"
                + (
                    "; an account name that is also an ordinary word is answered there too"
                    if any(item.kind == "name" for item in failed)
                    else ""
                )
            )
        return lines

    @property
    def notes(self) -> list:
        """Say once, for each kind of thing that is noted, where it was found.

        A name that is also a word is in many files, and a line for each would
        teach its reader to skip them.
        """
        lines = []
        for kind, what in (
            ("name", "this account's name"),
            ("home", "the shape of a home folder"),
        ):
            noted = [item for item in self.findings if item.kind == kind and not item.error]
            if noted:
                lines.append(
                    f"{what} is in {len(noted)} of the places read, the first being "
                    f"{noted[0].where}: {noted[0].text}. Noted and not failed on"
                )
        return lines

    @property
    def summary(self) -> str:
        """Say what a clean result rests on: what was read and what was looked for."""
        line = f"Read {self.files} file{'' if self.files == 1 else 's'}"
        if self.opened:
            line += (
                f", opening {self.opened} archive{'' if self.opened == 1 else 's'} or "
                f"compressed file{'' if self.opened == 1 else 's'} with {self.members} "
                f"member{'' if self.members == 1 else 's'}"
            )
        line += f", looking for {self.looked_for}."
        if self.skipped:
            line += " Not looked for: " + "; ".join(self.skipped) + "."
        return line


def shown(path) -> str:
    """Name a file in a report: from the current directory when it is under it.

    A symbolic link on the way is kept as it is written, so that a file reached
    through one is named by where it sits in what was given.
    """
    absolute = os.path.abspath(path)
    current = os.getcwd()
    if absolute.startswith(current + os.sep):
        return os.path.relpath(absolute, current)
    return str(path)


def times(count: int) -> str:
    return "once" if count == 1 else f"{count} times"


def excerpt(data: bytes, start: int, lead: int = 0) -> str:
    """Return the readable text at `start`, with up to `lead` bytes before it."""
    first = start
    while first > max(0, start - lead) and 0x20 <= data[first - 1] <= 0x7E:
        first -= 1
    kept = bytearray(data[first:start])
    after = data[start : start + 4 * EXAMPLE_LENGTH]
    if b"\0" in after[:4]:
        # UTF-16 or UTF-32: without its zero bytes, text of plain letters reads as it is.
        after = after.replace(b"\0", b"")
    for byte in after:
        if not 0x20 <= byte <= 0x7E:
            break
        kept.append(byte)
    return kept[: EXAMPLE_LENGTH + lead].decode("ascii")


def is_tar(data: bytes) -> bool:
    """Say whether `data` begins as a tar archive, with or without the `ustar` mark."""
    if data[257:262] == b"ustar":
        return True
    try:
        with tarfile.open(fileobj=io.BytesIO(data[:1024]), mode="r:") as archive:
            return archive.next() is not None
    except Exception:  # Whatever `tarfile` makes of bytes that are not a tar.
        return False


class Search:
    """One reading of some paths for one `Wanted`.

    `names_fail` says that what is read is to be published: the account's name
    and the shape of a home folder then fail, where otherwise they are noted.
    `name_is_a_word` goes back to noting the name alone.
    """

    def __init__(self, wanted: Wanted, names_fail: bool, name_is_a_word: bool = False) -> None:
        self.wanted = wanted
        self.names_fail = names_fail
        self.name_is_a_word = name_is_a_word
        self.report = Report(skipped=list(wanted.skipped))

        # Every way a location may be held, with the location's name. Where
        # two locations are written alike, the first listed gives the name.
        keys: dict = {}
        for name, spellings in wanted.locations:
            for spelling in spellings:
                for written in sorted({spelling, spelling.replace("\\", "/")}):
                    for encoding in ENCODINGS:
                        key = written.encode(encoding, "surrogateescape")
                        keys.setdefault(key.lower() if wanted.fold_case else key, name)
        # A location under another is found wherever the outer one is, so only
        # the outermost are searched for; the longest key at a hit names it.
        self.location_roots: dict = {}
        for key in sorted(keys, key=len):
            for root in self.location_roots:
                if key.startswith(root):
                    self.location_roots[root].insert(0, (key, keys[key]))
                    break
            else:
                self.location_roots[key] = [(key, keys[key])]

        # The name as the system gives it, in lower and upper case and with
        # capitals, each then folded as the bytes it is looked for in are: so
        # a name of plain letters is found in any case, and one with a letter
        # outside ASCII in those three.
        self.name_keys = sorted(
            {
                form.encode(encoding, "surrogateescape").lower()
                for name in wanted.names
                for form in (name, name.lower(), name.upper(), name.title())
                for encoding in ENCODINGS
            }
        )
        self.account = {name.casefold() for name in list(wanted.names) + list(wanted.owners)}
        self.own_paths = [path.encode("utf-8", "surrogateescape") for path in wanted.own_paths]

        count = len(wanted.locations)
        self.report.looked_for = (
            f"{count} location{'' if count == 1 else 's'} of this machine, {len(SHAPES)} shapes "
            "of a path that a build made elsewhere or without the flags leaves"
        )
        if wanted.names:
            self.report.looked_for += " and this account's name"
        if not names_fail:
            self.report.looked_for += " (a name or a home folder's shape is noted, not failed on)"
        elif name_is_a_word and wanted.names:
            self.report.looked_for += " (the name is noted, not failed on)"

    # Searching one run of bytes.

    def is_own_text(self, data: bytes, start: int, end: int) -> bool:
        """Say whether a hit lies inside text that is the build's, not the machine's.

        That is a path under one of the stand-ins, or one of the checkout's
        own relative paths exactly as it is written there.
        """
        low = max(0, start - STAND_IN_REACH)
        for opening in STAND_IN_STARTS:
            # Strictly before the hit: a location that itself begins `/cargo`
            # is not its own excuse.
            at = data.rfind(opening, low, start)
            while at >= 0:
                match = STAND_IN.match(data, at)
                if match and match.end() >= end:
                    return True
                at = data.rfind(opening, low, at)
        text = data[start:end]
        for path in self.own_paths:
            offset = path.find(text)
            while offset >= 0:
                if start >= offset and data.startswith(path, start - offset):
                    return True
                offset = path.find(text, offset + 1)
        return False

    def search(self, where: str, data: bytes) -> None:
        """Report what `data` holds, as found at `where`."""
        folded = data.lower() if self.name_keys or self.wanted.fold_case else data
        haystack = folded if self.wanted.fold_case else data

        counts: dict = {}
        for root, keys in self.location_roots.items():
            at = haystack.find(root)
            while at >= 0:
                key, name = next(item for item in keys if haystack.startswith(item[0], at))
                end = at + len(key)
                # A location is a whole folder: where it really is, a separator
                # or the end of the text follows it. Inside the build's own
                # text and stopping part-way through a name it is only that
                # text: `/src/app` in a dependency's `src/app.rs`, `/home/mo`
                # in the game's `src/ui/home/mod.rs`.
                whole = data[end : end + 1] in (b"/", b"\\", b"")
                if whole or not self.is_own_text(data, at, end):
                    if name not in counts:
                        counts[name] = [0, excerpt(data, at)]
                    counts[name][0] += 1
                at = haystack.find(root, at + 1)
        for name, (count, example) in sorted(counts.items(), key=lambda item: -item[1][0]):
            self.report.findings.append(
                Finding(
                    where,
                    "location",
                    f"{count} path{'' if count == 1 else 's'} of this machine under {name}, "
                    f"such as {example}",
                    True,
                )
            )

        if counts:
            # What else is here is mostly these paths seen another way, and
            # they fail the file by themselves; the rest shows once they are gone.
            return

        for description, openings, pattern, home in SHAPES:
            count = 0
            example = ""
            for opening in openings:
                at = data.find(opening)
                while at >= 0:
                    match = pattern.match(data, at)
                    # A home folder's shape is judged by where it begins. It
                    # ends at the next separator, which for a file in a folder
                    # called `home` comes from whatever is stored after it.
                    start = at + len(opening) + 1
                    if match and not (home and self.is_own_text(data, at, start)):
                        count += 1
                        example = example or excerpt(data, at, lead=40)
                    at = data.find(opening, at + 1)
            if count:
                self.report.findings.append(
                    Finding(
                        where,
                        "home" if home else "shape",
                        f"{description}, {times(count)}, such as {example}",
                        self.names_fail or not home,
                    )
                )

        count = sum(folded.count(key) for key in self.name_keys)
        if count:
            first = min(at for at in (folded.find(key) for key in self.name_keys) if at >= 0)
            self.report.findings.append(
                Finding(
                    where,
                    "name",
                    f"this account's name, {times(count)}, such as {excerpt(data, first, lead=30)}",
                    self.names_fail and not self.name_is_a_word,
                )
            )

    def search_text(self, where: str, text: str) -> None:
        self.search(where, text.encode("utf-8", "surrogateescape"))

    def names_account(self, owner: str) -> bool:
        """Say whether an archive member's owner or group is this account, exactly."""
        return owner.casefold() in self.account

    # Reading what the bytes are part of.

    def read_bytes(self, where: str, data: bytes, depth: int) -> None:
        """Search `data`, and what it holds if it is an archive or is compressed."""
        self.search(where, data)

        for description, recognises in OPAQUE:
            if recognises(data):
                self.report.problems.append(
                    f"{where}: is or holds {description}, which this check cannot read into; "
                    + REFUSED
                )
                return
        if zipfile.is_zipfile(io.BytesIO(data)):
            description, reader = "a zip archive", self.read_zip
        elif is_tar(data):
            description, reader = "a tar archive", self.read_tar
        else:
            for description, recognises, module in PACKED:
                if recognises(data):
                    break
            else:
                return  # No format known here: it has been read as the bytes it is.
            if self.wanted.openers[module] is None:
                self.report.problems.append(
                    f"{where}: is {description}, which this Python cannot open, having no "
                    f"`{module}` module; " + REFUSED
                )
                return
            reader = self.read_packed(self.wanted.openers[module])
        if depth >= MAXIMUM_DEPTH:
            self.report.problems.append(f"{where}: is {description} too deep in others to read")
            return
        self.report.opened += 1
        try:
            reader(where, data, depth + 1)
        except Exception as error:  # Each format raises its own; none is a clean result.
            self.report.problems.append(
                f"{where}: looks like {description} and cannot be read: {error}"
            )

    def read_packed(self, module):
        """Return a reader for one compressed stream, which is read as what it holds."""

        def reader(where: str, data: bytes, depth: int) -> None:
            self.read_bytes(where, module.decompress(data), depth)

        return reader

    # An archive keeps its own text - member names, link targets, comments,
    # extended headers - as plain bytes, which `read_bytes` has searched by the
    # time one of these is called. What is left is what those bytes hide: each
    # member's contents, and whose the members are.

    def read_zip(self, where: str, data: bytes, depth: int) -> None:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            for member in archive.infolist():
                if member.is_dir():
                    continue
                inside = f"{where}: {member.filename}"
                if member.flag_bits & 0x1:
                    self.report.problems.append(f"{inside}: is encrypted and cannot be read")
                    continue
                self.report.members += 1
                self.read_bytes(inside, archive.read(member), depth)

    def read_tar(self, where: str, data: bytes, depth: int) -> None:
        owned = []
        # `ignore_zeros` reads on past an end-of-archive mark, which is all
        # that separates two archives written one after the other.
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:", ignore_zeros=True) as archive:
            for member in archive:
                if self.names_account(member.uname) or self.names_account(member.gname):
                    owned.append(member.name)
                if member.isfile():
                    content = archive.extractfile(member)
                    self.report.members += 1
                    self.read_bytes(
                        f"{where}: {member.name}", content.read() if content else b"", depth
                    )
        if owned:
            # Nothing else about the name is this certain. In a file, a name
            # that is also a word may be there by chance; as the owner or the
            # group of an archived file it is the account, so this always fails.
            self.report.findings.append(
                Finding(
                    where,
                    "owner",
                    f"names this account as the owner or group of {len(owned)} of its members, "
                    f"such as {owned[0]}",
                    True,
                )
            )

    def read_path(self, path: Path, seen: set) -> None:
        """Read a file, or everything under a folder, following symbolic links."""
        where = shown(path)
        self.search_text(f"{where} (its name)", path.name)
        try:
            if path.is_symlink():
                self.search_text(f"{where} (the link's target)", os.readlink(path))
                if not path.exists():
                    self.report.problems.append(f"{where}: is a link to something that is not there")
                    return
            if path.is_dir():
                real = os.path.realpath(path)
                if real in seen:
                    return
                seen.add(real)
                for child in sorted(path.iterdir()):
                    self.read_path(child, seen)
            elif path.is_file():
                data = path.read_bytes()
                self.report.files += 1
                self.read_bytes(where, data, 0)
            else:
                self.report.problems.append(f"{where}: is neither a file nor a folder")
        except OSError as error:
            self.report.problems.append(f"{where}: cannot be read: {error}")

    def read(self, paths) -> Report:
        for path in paths:
            before = self.report.files
            if not Path(path).exists() and not Path(path).is_symlink():
                self.report.problems.append(f"{shown(path)}: not found")
                continue
            self.read_path(Path(path), set())
            if self.report.files == before:
                self.report.problems.append(f"{shown(path)}: holds no file, so nothing was read")
        return self.report


# The check's own cases.
#
# An invented machine, the same wherever this runs, so that no case depends on
# what this machine is called or where it keeps things. A case says who must
# fail on it: the check of what is published, that check told the name is a
# word, and a build script.

PLANTED = Wanted(
    locations=[
        ("the checkout", ["/planted/checkout"]),
        ("the home folder", ["/planted-home/mo"]),
        ("Cargo's home", ["/planted-home/mo/.cargo"]),
        ("a location that begins as a stand-in does", ["/cargo-planted/tools"]),
    ],
    names=["plantedname", "pl\u00e4nt\u00e9d", "\u043f\u043b\u0430\u043d\u0442"],
    own_paths=[
        "src/ui/planted-home/mod.rs",
        "src/ui/home/planted/widget.rs",
        "src/ui/home/planted.rs",
    ],
    fold_case=False,
    owners=["p"],
)
PLANTED_WINDOWS = Wanted(
    locations=[("the checkout", ["C:\\Users\\Planted Name\\game"])],
    names=["Planted Name"],
    fold_case=True,
)
# The same machine with a Python that was built with no module for any of them.
PLANTED_BARE = Wanted(locations=PLANTED.locations, openers=dict.fromkeys(MODULES))
PLANTED_ENCODINGS = ("utf-32-be", "utf-32-le", "utf-16-be", "utf-16-le", "utf-8")
REGISTRY = b"/cargo/registry/src/index.crates.io-0123456789abcdef/planted-1.0.0/src"
ALWAYS = (True, True, True)
NEVER = (False, False, False)
NAME = (True, False, False)
HOME = (True, True, False)
MODES = (
    ("the check of what is published", {"names_fail": True}),
    ("that check, told the name is a word", {"names_fail": True, "name_is_a_word": True}),
    ("a build script", {"names_fail": False}),
)


def tar_of(*members, form=tarfile.PAX_FORMAT) -> bytes:
    packed = io.BytesIO()
    with tarfile.open(fileobj=packed, mode="w", format=form) as archive:
        for member, content in members:
            member.size = len(content)
            archive.addfile(member, io.BytesIO(content))
    return packed.getvalue()


def tar_member(name: str = "folder/inside", owner: str = "", group: str = "", link: str = ""):
    member = tarfile.TarInfo(name)
    member.uname, member.gname = owner, group
    if link:
        member.type, member.linkname = tarfile.SYMTYPE, link
    return member


def old_tar_of(name: str, content: bytes) -> bytes:
    """Return a tar of one member in the oldest format, which has no `ustar` mark."""
    data = bytearray(tar_of((tar_member(name), content), form=tarfile.USTAR_FORMAT))
    data[257:329] = b"\x00" * 72  # The mark, its version, and the owner's and group's names.
    data[148:156] = b" " * 8
    data[148:156] = b"%06o\x00 " % sum(data[:512])
    return bytes(data)


def zip_of(name: str, content: bytes, comment: bytes = b"") -> bytes:
    packed = io.BytesIO()
    with zipfile.ZipFile(packed, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr(name, content)
        archive.comment = comment
    return packed.getvalue()


def plant(root: Path, wanted: Wanted) -> list:
    """Write every case under `root`.

    Returns, for each, what it shows, where it is, who must fail on it, and
    which machine it is read as. The encodings and the formats are written out
    here and not taken from the tables the search uses, so that one dropped
    from a table is missed, where otherwise it would only go unplanted.
    """
    cases: list = []
    gzip, bz2, lzma, zstd = (
        MODULES[module] for module in ("gzip", "bz2", "lzma", "compression.zstd")
    )
    path = b"/planted/checkout"

    def folder() -> Path:
        # Names that no account can have, so that no case is found by its own.
        made = root / f"{len(cases):03}" / "_"
        made.mkdir(parents=True, exist_ok=True)
        return made

    def case(shows: str, fails: tuple, content: bytes, machine=PLANTED, name: str = "_") -> None:
        made = folder()
        (made / name).write_bytes(content)
        cases.append((shows, made, fails, machine))

    # A location, in each way it may be held.
    for encoding in PLANTED_ENCODINGS:
        held = b"\x01" + "/planted/checkout/src/x.rs".encode(encoding)
        case(f"a location as {encoding}", ALWAYS, held)
    case("a location that runs into the text after it", ALWAYS, b"lib.rs" + path + b"assertion")
    case("a location under another", ALWAYS, b"/planted-home/mo/.cargo/bin/tool")
    case("a location that begins as a stand-in does", ALWAYS, b"\x00/cargo-planted/tools/bin/x")
    for written in ("C:\\Users\\Planted Name\\game\\x.rs", "c:/users/planted name/game/x.rs"):
        shows = "a Windows location, in either case and with either slash"
        case(shows, ALWAYS, written.encode(), PLANTED_WINDOWS)
    held = "C:\\USERS\\PLANTED NAME\\GAME".encode("utf-16-le")
    case("a Windows location as UTF-16", ALWAYS, held, PLANTED_WINDOWS)

    # The build's own text, and beside each piece of it the nearest thing that is not.
    case("one of the checkout's own paths", NEVER, b"\x00src/ui/planted-home/mod.rs\x00")
    case("a path that is nearly one of the checkout's own", ALWAYS, b"\x00src/ui/planted-home/mop.rs")
    case("a location after other text", ALWAYS, b"lib.rs/planted-home/mo/notes.txt")
    case("a stand-in path that holds a location's text", NEVER, REGISTRY + b"/planted-home/mod.rs")
    case("a stand-in path that holds a whole location", ALWAYS, REGISTRY + b"/planted-home/mo/d.rs")
    case("a stand-in path like a location's beginning", NEVER, REGISTRY + b"/cargo-planted/tools.rs")
    held = REGISTRY + b"/lib.rs" + path + b"x.rs"
    case("a location after a stand-in path that has ended", ALWAYS, held)
    held = REGISTRY + b"/keywords.in" + path + b"/tests/out/gen.rs"
    case("a location after a stand-in path with no .rs", ALWAYS, held)
    # `STAND_IN` allows 200 bytes between a stand-in and the `.rs` that ends its path.
    room = 200 - len(REGISTRY[len(b"/cargo/registry/") :] + b"/" + b"/planted-home/mod")
    held = REGISTRY + b"/" + b"a" * room + b"/planted-home/mod.rs"
    case("a stand-in path as long as one may be", NEVER, held)
    held = REGISTRY + b"/" + b"a" * (room + 1) + b"/planted-home/mod.rs"
    case("a location further from a stand-in than a path is long", ALWAYS, held)
    case(
        "every stand-in",
        NEVER,
        REGISTRY
        + b"/lib.rs\x00/cargo/git/checkouts/planted-0123456789abcdef/abc1234/src/lib.rs\x00"
        b"/rustc/0123456789abcdef0123456789abcdef01234567/library/core/src/fmt/mod.rs\x00"
        b"/rust/deps/planted-1.0.0/src/lib.rs\x00/cargo/build-dir/release/build/x/out/y.rs\x00"
        b"/cargo/deps/planted-1.0.0/src/lib.rs",
    )

    # The shapes of a build made elsewhere, or without the flags.
    index = b"index.crates.io-0123456789abcdef"
    for shows, held in (
        ("a registry path", b"/opt/shared/registry/src/" + index + b"/x-1.0.0/src/lib.rs"),
        ("a Windows registry path", b"D:\\shared\\registry\\src\\" + index + b"\\x-1.0.0\\src\\lib.rs"),
        ("a git dependency's path", b"/opt/shared/git/checkouts/x-0123456789abcdef/abc/src/lib.rs"),
        ("the standard library's source", b"/opt/rust/lib/rustlib/src/rust/library/core/src/x.rs"),
        ("a rustup toolchain", b"/opt/tools/.rustup/toolchains/stable-planted/lib/librustc.so"),
    ):
        case(f"{shows} with no stand-in", ALWAYS, held)
    held = b"/usr/local/cargo/registry/src/" + index + b"/x-1.0.0/src/lib.rs"
    case("a registry path under a folder called cargo, the known blind spot", NEVER, held)
    held = b"D:\\cargo\\registry\\src\\" + index + b"\\x-1.0.0\\src\\lib.rs"
    case("a Windows registry path under a folder called cargo", ALWAYS, held)
    for held in (b"/Users/somebody/Library/x", b"/home/somebody/.config/x", b"C:\\Users\\Some Body\\x"):
        case("the shape of a home folder", HOME, b"\x00" + held)
    case("a home folder's shape in one of the checkout's own paths", NEVER, b"src/ui/home/planted/widget.rs")
    case("a home folder's shape in a stand-in path", NEVER, REGISTRY + b"/home/planted/mod.rs")
    case("a file called home", NEVER, b"src/ui/home/mod.rs\x00/home/mod.rs")
    # A file in a folder called `home`, with whatever is stored next supplying the separator.
    own = b"src/ui/home/planted.rs"
    case("an own path under a folder called home, then another path", NEVER, own + b"src/ui/hud.rs")
    case("an own path under a folder called home, then a stand-in path", NEVER, own + REGISTRY + b"/x.rs")
    case("an own path under a folder called home, then a backslash", NEVER, own + b"\\next")
    case("a stand-in path under a folder called home, then text", NEVER, REGISTRY + b"/home/x.rsmore/text")
    case("a path that is nearly an own path under home", HOME, b"lib/ui/home/planted.rssrc/ui/")
    case("a home folder after an own path under home", HOME, own + b"\x00/home/somebody/.config/x")
    case("a home folder straight after an own path", HOME, own + b"/home/somebody/.config/x")

    # The account's name.
    for name in PLANTED.names:
        for form in (name, name.upper(), name.title()):
            for encoding in PLANTED_ENCODINGS:
                case(f"the name as {encoding}", NAME, b"built by " + form.encode(encoding) + b" on")
    case("a name with a space, on Windows", NAME, b"signed by PLANTED name", PLANTED_WINDOWS)
    case("the name in a file's name", NAME, b"clean", name="plantedname-notes.txt")
    case("the name in a zip member's name", NAME, zip_of("folder/plantedname-notes.txt", b"clean"))
    case("a file that does not hold the name", NEVER, b"planted name, planted-name, plantednam")
    for shows, fails, member in (
        ("a tar member owned by the account", ALWAYS, tar_member(owner="PlantedName")),
        ("a tar member in the account's group", ALWAYS, tar_member(group="plantedname")),
        ("a tar member owned by an account of one letter", ALWAYS, tar_member(owner="p")),
        ("a tar owner that holds the name and more", NAME, tar_member(owner="xplantednamey")),
        ("a tar member owned by somebody else", NEVER, tar_member(owner="pp", group="staff")),
    ):
        case(shows, fails, tar_of((member, b"clean")))

    # Archives, and what is refused.
    case("a zip member", ALWAYS, zip_of("folder/inside", b"in a zip " + path))
    case("a zip comment", ALWAYS, zip_of("folder/inside", b"clean", comment=path))
    case("a zip behind the program that unpacks it", ALWAYS, b"#!/bin/sh\nexit 0\n" + zip_of("f", path))
    held = zip_of("inside", b"clean")
    for _ in range(MAXIMUM_DEPTH):
        held = zip_of("inside.zip", held)
    case("a zip too deep in others to read", ALWAYS, held)
    case("a zip that is clean", NEVER, zip_of("folder/inside", b"clean"))
    first = tar_of((tar_member(), b"clean"))
    held = first + tar_of((tar_member("second/inside"), b"in the second tar " + path))
    case("the second of two tars joined", ALWAYS, held)
    held = first + tar_of((tar_member(owner="plantedname"), b"clean"))
    case("the owner of the second of two tars joined", ALWAYS, held)
    held = tar_of((tar_member("folder/link", link="/planted/checkout/x"), b""))
    case("a tar link's target", ALWAYS, held)
    member = tar_member()
    member.pax_headers = {"comment": "made in /planted/checkout"}
    case("a tar member's extended header", ALWAYS, tar_of((member, b"clean")))
    # Long and repetitive, so that each format compresses it and none stores it as it is.
    body = (b"inside " + path + b" ") * 40
    if gzip:
        inner = gzip.compress(body)
        case("a gzip file", ALWAYS, inner)
        held = gzip.compress(tar_of((tar_member("folder/inside.gz"), inner)))
        case("a gzip member of a gzip tar", ALWAYS, held)
        case("a gzip member of a tar with no ustar mark", ALWAYS, old_tar_of("inside.gz", inner))
        case("a gzip file cut short", ALWAYS, inner[: len(inner) // 2])
        named = io.BytesIO()
        with gzip.GzipFile(filename="plantedname-build.log", mode="wb", fileobj=named) as stream:
            stream.write(b"clean")
        case("the name of the file a gzip file was made from", NAME, named.getvalue())
    skipped = b"\x50\x2a\x4d\x18\x04\x00\x00\x00skip"
    if bz2:
        case("a bzip2 file", ALWAYS, bz2.compress(body))
    if lzma:
        case("an xz file", ALWAYS, lzma.compress(body))
        case("a file in the older lzma format", ALWAYS, lzma.compress(body, format=lzma.FORMAT_ALONE))
    if zstd:
        case("a zstd file", ALWAYS, zstd.compress(body))
        case("a zstd file that begins with a frame to skip", ALWAYS, skipped + zstd.compress(body))
    for shows, held in (
        ("a gzip file", b"\x1f\x8b\x08"),
        ("a bzip2 file", b"BZh91AY&SY"),
        ("an xz file", b"\xfd7zXZ\x00"),
        ("a file in the older lzma format", b"\x5d\x00\x00\x80\x00" + b"\xff" * 8),
        ("a zstd file", b"\x28\xb5\x2f\xfd"),
        ("a zstd file that begins with a frame to skip", skipped + b"\x28\xb5\x2f\xfd"),
    ):
        shows += ", for a Python with no module to open it"
        case(shows, ALWAYS, held + b"\x00" * 32, PLANTED_BARE)
    for shows, held in (
        ("a 7z archive", b"7z\xbc\xaf\x27\x1c" + b"\x00" * 32),
        ("a 7z archive behind the program that unpacks it", b"MZ\x90\x00stub7z\xbc\xaf\x27\x1c"),
        ("a RAR archive", b"Rar!\x1a\x07\x00" + b"\x00" * 32),
        ("an installer database", b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1" + b"\x00" * 32),
        ("a cpio archive", b"070707" + b"0" * 70),
        ("a disk image", b"x" * 2000 + b"koly" + b"\x00" * 508),
    ):
        case(f"{shows}, which cannot be read into", ALWAYS, held)

    # Folders and links.
    cases.append(("a folder with no file in it", folder(), ALWAYS, PLANTED))
    case("a file that is clean", NEVER, b"clean")
    behind = root / "behind"
    behind.mkdir()
    (behind / "_").write_bytes(b"behind a link " + path)
    clean = root / "clean"
    clean.mkdir()
    (clean / "_").write_bytes(b"clean")
    # A machine whose one location is a folder that is really there, so that a
    # link can be to something that exists and still have a location as its text.
    linked = Wanted(locations=[("a folder that is really there", [str(clean)])])
    try:
        for shows, fails, target, machine in (
            ("a file behind a link to a folder", ALWAYS, behind, PLANTED),
            ("a link whose own target is a location", ALWAYS, clean / "_", linked),
            ("a link to something clean", NEVER, clean / "_", PLANTED),
            ("a link to nothing", ALWAYS, root / "nothing-here", PLANTED),
        ):
            made = folder()
            (made / "_").write_bytes(b"clean")
            os.symlink(target, made / "-")
            cases.append((shows, made, fails, machine))
    except (OSError, NotImplementedError):
        pass  # Not every system lets every account make a link.

    # This machine, plainly: each of its locations and names, with nothing clever around it.
    for name, spellings in wanted.locations:
        for spelling in spellings:
            held = b"\x00" + (spelling + "/planted.rs").encode("utf-8", "surrogateescape")
            case(f"this machine's own location ({name})", ALWAYS, held, wanted)
    for name in wanted.names:
        held = b"\x00" + name.encode("utf-8", "surrogateescape") + b"\x00"
        case("this account's own name", NAME, held, wanted)
    for name in list(wanted.names) + list(wanted.owners):
        held = tar_of((tar_member(owner=name), b"clean"))
        case("a tar member owned by this account", ALWAYS, held, wanted)
    return cases


def self_test(wanted: Wanted) -> list:
    """Plant the check's own cases, and judge each by whether the check would fail.

    Returns a line for each case that came out wrong, and nothing when all are
    right. Every case is read by the code that reads the real files and judged
    by the errors that fail the real commands, so a change that stops a rule
    failing, or lets an exemption excuse too much, fails here on the next run,
    where it would otherwise pass every build.
    """
    failures = []
    with tempfile.TemporaryDirectory() as folder:
        for shows, path, fails, machine in plant(Path(folder), wanted):
            for (who, mode), must_fail in zip(MODES, fails):
                failed = bool(Search(machine, **mode).read([path]).errors)
                if failed != must_fail:
                    failures.append(
                        f"{who} {'fails on' if failed else 'passes'} {shows}, "
                        f"and must {'pass' if failed else 'fail on'} it"
                    )
    return failures
