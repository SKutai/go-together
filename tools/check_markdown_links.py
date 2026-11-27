#!/usr/bin/env python3
"""Validate repository-local Markdown paths and heading fragments.

The repository is what git lists for this checkout with
`git ls-files --cached --others --exclude-standard`: every tracked file, and
every untracked one that no ignore rule covers, so a new document is checked
before it is added. Those are the Markdown files read, and the only files a
link may name. A file that `.gitignore`, `.git/info/exclude` or a global
excludes file covers is neither, even when it is on this disk, because the
repository does not contain it. Nothing is found by walking the disk, so
build output, nested checkouts and anything else git ignores need no list of
their own here.

A link is resolved the way a URL is, from the directory of the file it is in
and without consulting the disk, and must then name a listed file exactly,
case included, as it must on a case-sensitive file system. A link to a
directory names that directory's `README.md`. A symbolic link is neither read
as Markdown nor accepted as a target: git stores one as the path it points
to, so that path, not a document, is what a link to it would reach.

Two kinds of link are written as a full address and still name a file here,
so they are checked like the rest instead of being passed over as external.
One is the address GitHub shows a file of this repository's default branch
at, `https://github.com/<owner>/<name>/blob/main/<path>`: it is resolved from
the repository's root, wherever it is written. A full address of anything
else - an issue, another branch, a commit - is not a file in this checkout
and is not checked.

The other is every link in an issue form, the YAML files in
`.github/ISSUE_TEMPLATE/` other than `config.yml`. A form's text is Markdown,
so its links are read the same way, but GitHub shows a form on the page for a
new issue and not among the repository's files, where a relative link leads
nowhere. A form therefore has to give a file's full address, and a relative
link in one is an error.

Outside a git checkout of its own - in an exported copy, say, or one unpacked
inside another repository - there is no list to check against, so the check
stops with an error rather than guessing.
"""

from __future__ import annotations

from functools import lru_cache
from html import unescape
import os
from pathlib import Path
import posixpath
import re
import subprocess
import sys
from urllib.parse import SplitResult, parse_qs, unquote, urlsplit


ROOT = Path(__file__).resolve().parents[1]
# Where GitHub shows the files of this repository's default branch. GitHub
# compares the host, the owner and the name without regard to case; the rest
# of an address is a path, and is matched exactly.
REPOSITORY = ("github.com", "skutai", "go-together")
DEFAULT_BRANCH_VIEWS = (("blob", "main"), ("tree", "main"))
REPOSITORY_FILE_ADDRESS = "https://github.com/SKutai/go-together/blob/main/"
ISSUE_FORM_DIRECTORY = ".github/ISSUE_TEMPLATE/"
INLINE_LINK = re.compile(
    r"!?\[[^\]]*]\(\s*(?P<target><[^>]+>|[^)\s]+)(?:\s+[^)]*)?\)",
)
REFERENCE_LINK = re.compile(r"^\s{0,3}\[[^\]]+]:\s*(?P<target><[^>]+>|[^\s]+)")
HTML_LINK = re.compile(r"<(?:a|img)\b[^>]*\b(?:href|src)=[\"'](?P<target>[^\"']+)[\"']", re.I)
ATX_HEADING = re.compile(r"^\s{0,3}#{1,6}\s+(?P<heading>.*?)\s*#*\s*$")
SETEXT_HEADING = re.compile(r"^\s{0,3}(?:=+|-+)\s*$")
HTML_ID = re.compile(r"\bid=[\"'](?P<id>[^\"']+)[\"']", re.I)
MARKUP = re.compile(r"[`*_~]")
HTML_TAG = re.compile(r"<[^>]+>")


def markdown_without_code(text: str) -> str:
    output: list[str] = []
    fence: str | None = None
    for line in text.splitlines():
        marker = re.match(r"^\s{0,3}(`{3,}|~{3,})", line)
        if marker:
            token = marker.group(1)
            if fence is None:
                fence = token[0]
            elif token[0] == fence:
                fence = None
            output.append("")
            continue
        if fence is not None:
            output.append("")
            continue
        output.append(re.sub(r"`[^`]*`", "", line))
    return "\n".join(output)


def github_slug(heading: str) -> str:
    heading = unescape(HTML_TAG.sub("", MARKUP.sub("", heading))).lower()
    characters = [
        character
        for character in heading
        if character.isalnum() or character in {" ", "-", "_"}
    ]
    return "".join(characters).strip().replace(" ", "-")


@lru_cache(maxsize=None)
def anchors(path: Path) -> set[str]:
    text = path.read_text(encoding="utf-8")
    result = {unquote(match.group("id")).lower() for match in HTML_ID.finditer(text)}
    counts: dict[str, int] = {}
    lines = markdown_without_code(text).splitlines()

    headings: list[str] = []
    for index, line in enumerate(lines):
        match = ATX_HEADING.match(line)
        if match:
            headings.append(match.group("heading"))
        elif index > 0 and SETEXT_HEADING.match(line) and lines[index - 1].strip():
            headings.append(lines[index - 1].strip())

    for heading in headings:
        base = github_slug(heading)
        if not base:
            continue
        duplicate = counts.get(base, 0)
        result.add(base if duplicate == 0 else f"{base}-{duplicate}")
        counts[base] = duplicate + 1
    return result


def targets(text: str) -> list[str]:
    cleaned = markdown_without_code(text)
    found = [match.group("target") for match in INLINE_LINK.finditer(cleaned)]
    found.extend(
        match.group("target")
        for line in cleaned.splitlines()
        if (match := REFERENCE_LINK.match(line))
    )
    found.extend(match.group("target") for match in HTML_LINK.finditer(cleaned))
    return [target[1:-1] if target.startswith("<") and target.endswith(">") else target for target in found]


def git(*arguments: str) -> bytes:
    try:
        completed = subprocess.run(["git", "-C", str(ROOT), *arguments], capture_output=True)
    except FileNotFoundError:
        raise SystemExit("error: git was not found, and this check takes the repository's files from git")
    if completed.returncode != 0:
        reason = completed.stderr.decode(errors="replace").strip()
        raise SystemExit(f"error: this check takes the repository's files from git, which failed in {ROOT}: {reason}")
    return completed.stdout


def repository_files() -> frozenset[str]:
    """The checkout's files as git lists them, `/`-separated from ROOT."""
    if prefix := os.fsdecode(git("rev-parse", "--show-prefix")).strip():
        raise SystemExit(
            f"error: {ROOT} is the folder {prefix} of another git checkout rather than a checkout "
            "of its own, so git cannot say which files are this repository's"
        )
    listed = git("ls-files", "-z", "--cached", "--others", "--exclude-standard")
    # A tracked file deleted from the disk but not yet from the index is still
    # listed, and is not there to link to.
    return frozenset(
        path
        for path in (os.fsdecode(entry) for entry in listed.split(b"\0") if entry)
        if os.path.lexists(ROOT / path)
    )


def is_issue_form(path: str) -> bool:
    """Whether `path` is an issue form: YAML in the forms' folder, apart from
    `config.yml`, which configures the page that offers them and holds no
    Markdown."""
    folder, _, name = path.rpartition("/")
    stem, _, extension = name.rpartition(".")
    return f"{folder}/" == ISSUE_FORM_DIRECTORY and extension in {"yml", "yaml"} and stem != "config"


def own_file(parsed: SplitResult) -> str | None:
    """The path, from the repository's root, that a full address names.

    `None` unless the address is one GitHub shows a file or a folder of this
    repository's default branch at. The path is returned as written, still
    percent-encoded and not yet normalized.
    """
    if parsed.scheme not in {"http", "https"}:
        return None
    host, owner, name = REPOSITORY
    segments = parsed.path.split("/")
    if (
        parsed.netloc.lower() != host
        or len(segments) < 6
        or segments[0] != ""
        or (segments[1].lower(), segments[2].lower()) != (owner, name)
        or (segments[3], segments[4]) not in DEFAULT_BRANCH_VIEWS
    ):
        return None
    return "/".join(segments[5:]) or None


def validate(source: str, target: str, files: frozenset[str]) -> str | None:
    parsed = urlsplit(target)
    if (own := own_file(parsed)) is not None:
        name = posixpath.normpath(unquote(own))
    elif parsed.scheme or parsed.netloc:
        return None
    elif is_issue_form(source):
        return (
            "relative link in an issue form, which GitHub does not show among the "
            f"repository's files; write the full address, {REPOSITORY_FILE_ADDRESS}<path>: {target}"
        )
    elif target.startswith(("/", "\\")):
        return f"root-absolute local link is not portable: {target}"
    else:
        relative_path = unquote(parsed.path)
        name = (
            posixpath.normpath(posixpath.join(posixpath.dirname(source), relative_path))
            if relative_path
            else source
        )
    if name == ".." or name.startswith("../"):
        return f"local link escapes the repository: {target}"

    candidates = (name, posixpath.normpath(posixpath.join(name, "README.md")))
    destination = next((path for path in candidates if path in files), None)
    if destination is None:
        for candidate in candidates:
            spelled = next((path for path in files if path.lower() == candidate.lower()), None)
            if spelled is not None:
                return f"local link target is spelled {spelled} in the repository: {target}"
        folder = "" if name == "." else f"{name}/"
        if any(path.startswith(folder) for path in files):
            return f"local link names a directory with no README.md: {target}"
        if os.path.lexists(ROOT / name):
            return f"local link target is on this disk but not in the repository: {target}"
        return f"missing local link target: {target}"
    if (ROOT / destination).is_symlink():
        return f"local link target is a symbolic link; link to the file it points to: {target}"

    # `?plain=1` asks GitHub for a Markdown file's source, where a fragment
    # names lines and not a heading.
    if "1" in parse_qs(parsed.query).get("plain", []):
        return None
    fragment = unquote(parsed.fragment).lower()
    if fragment and destination.lower().endswith(".md") and fragment not in anchors(ROOT / destination):
        return f"missing heading '#{fragment}' in {destination}"
    return None


def main() -> None:
    files = repository_files()
    markdown_files = sorted(
        path for path in files if path.endswith(".md") and not (ROOT / path).is_symlink()
    )
    if not markdown_files:
        raise SystemExit(f"error: git lists no Markdown files in {ROOT}, so there is nothing to check")
    # An issue form is YAML whose text GitHub renders as Markdown, so its
    # links are found the way a Markdown file's are.
    issue_forms = sorted(
        path for path in files if is_issue_form(path) and not (ROOT / path).is_symlink()
    )
    errors: list[str] = []
    checked = 0

    for source in markdown_files + issue_forms:
        for target in targets((ROOT / source).read_text(encoding="utf-8")):
            checked += 1
            if error := validate(source, target, files):
                errors.append(f"{source}: {error}")

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)

    forms = "1 issue form" if len(issue_forms) == 1 else f"{len(issue_forms)} issue forms"
    print(f"Checked {checked} Markdown links across {len(markdown_files)} Markdown files and {forms}.")


if __name__ == "__main__":
    main()
