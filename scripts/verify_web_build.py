#!/usr/bin/env python3
"""Verify that a Trunk distribution is self-contained and itch.io-ready.

That includes holding no path of the machine that built it, which
`machine_paths.py` checks.
"""

from __future__ import annotations

import argparse
from html.parser import HTMLParser
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

import machine_paths


RESOURCE_ATTRIBUTES = {
    "audio": ("src",),
    "embed": ("src",),
    "iframe": ("src",),
    "img": ("src", "srcset"),
    "input": ("src",),
    "link": ("href",),
    "object": ("data",),
    "script": ("src", "data-bindings", "data-wasm"),
    "source": ("src", "srcset"),
    "track": ("src",),
    "video": ("poster", "src"),
}
CSS_URL = re.compile(r"url\(\s*(['\"]?)(.*?)\1\s*\)", re.IGNORECASE)
JS_ROOT_URL = re.compile(
    r"(?:fetch|import|new\s+URL)\s*\(\s*(['\"])/(?!/)",
    re.IGNORECASE,
)
JS_NETWORK_URL = re.compile(
    r"(?:fetch|import|new\s+URL)\s*\(\s*(['\"])(?:https?:)?//",
    re.IGNORECASE,
)


def resource_candidates(value: str, attribute: str) -> list[str]:
    if attribute != "srcset":
        return [value]
    return [candidate.strip().split()[0] for candidate in value.split(",") if candidate.strip()]


def local_resource_path(url: str) -> str | None:
    value = url.strip()
    if not value or value.startswith("#"):
        return None
    if value.startswith(("/", "\\")):
        raise ValueError(f"root-absolute resource URL: {value}")

    parsed = urlsplit(value)
    if parsed.scheme or parsed.netloc:
        raise ValueError(f"network or scheme-qualified resource URL: {value}")
    return unquote(parsed.path)


class DistributionHTMLParser(HTMLParser):
    def __init__(self, document: Path, distribution: Path) -> None:
        super().__init__(convert_charrefs=True)
        self.document = document
        self.distribution = distribution
        self.errors: list[str] = []

    def handle_starttag(
        self,
        tag: str,
        attrs: list[tuple[str, str | None]],
    ) -> None:
        attributes = {name.lower(): value for name, value in attrs}
        tag = tag.lower()

        if tag == "style":
            self.errors.append("inline <style> element")
        if "style" in attributes:
            self.errors.append(f"inline style attribute on <{tag}>")
        if tag == "script" and not attributes.get("src"):
            self.errors.append("inline <script> element")

        for attribute in RESOURCE_ATTRIBUTES.get(tag, ()):
            value = attributes.get(attribute)
            if value is None:
                continue
            for candidate in resource_candidates(value, attribute):
                try:
                    path = local_resource_path(candidate)
                except ValueError as error:
                    self.errors.append(str(error))
                    continue
                if path is None:
                    continue

                resolved = (self.document.parent / path).resolve()
                try:
                    resolved.relative_to(self.distribution.resolve())
                except ValueError:
                    self.errors.append(f"resource escapes dist: {candidate}")
                    continue
                if not resolved.is_file():
                    self.errors.append(f"missing resource: {candidate}")


def fail(errors: list[str]) -> None:
    if not errors:
        return
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    raise SystemExit(1)


def verify(distribution: Path, prove: bool) -> None:
    errors: list[str] = []
    if not distribution.is_dir():
        fail([f"distribution directory not found: {distribution}"])

    index = distribution / "index.html"
    if not index.is_file():
        errors.append("dist/index.html is missing")

    root_js = sorted(path for path in distribution.glob("*.js") if path.is_file())
    root_wasm = sorted(path for path in distribution.glob("*.wasm") if path.is_file())
    if not root_js:
        errors.append("no generated JavaScript file exists at the dist root")
    if not root_wasm:
        errors.append("no generated WebAssembly file exists at the dist root")

    repository = Path(__file__).resolve().parents[1]
    source_wavs = {
        path.relative_to(repository / "assets")
        for path in (repository / "assets" / "audio").glob("*.wav")
    }
    output_wavs = {
        path.relative_to(distribution / "assets")
        for path in (distribution / "assets" / "audio").glob("*.wav")
    }
    if not source_wavs:
        errors.append("no source WAV files were found under assets/audio")
    missing_wavs = sorted(source_wavs - output_wavs)
    if missing_wavs:
        errors.append(
            "copied assets/audio is missing: "
            + ", ".join(path.as_posix() for path in missing_wavs)
        )

    for resource in ("styles.css", "shell.js"):
        if not (distribution / "web" / resource).is_file():
            errors.append(f"copied web shell resource is missing: web/{resource}")
    if not (distribution / "LICENSE").is_file():
        errors.append("copied MIT license is missing: LICENSE")

    if index.is_file():
        parser = DistributionHTMLParser(index, distribution)
        parser.feed(index.read_text(encoding="utf-8"))
        parser.close()
        errors.extend(f"index.html: {error}" for error in parser.errors)

    for css in distribution.rglob("*.css"):
        text = css.read_text(encoding="utf-8")
        for match in CSS_URL.finditer(text):
            try:
                path = local_resource_path(match.group(2))
            except ValueError as error:
                errors.append(f"{css.relative_to(distribution)}: {error}")
                continue
            if path:
                resolved = (css.parent / path).resolve()
                try:
                    resolved.relative_to(distribution.resolve())
                except ValueError:
                    errors.append(
                        f"{css.relative_to(distribution)}: resource escapes dist: {path}"
                    )
                    continue
                if not resolved.is_file():
                    errors.append(
                        f"{css.relative_to(distribution)}: missing resource: {path}"
                    )

    for javascript in distribution.rglob("*.js"):
        text = javascript.read_text(encoding="utf-8")
        if JS_ROOT_URL.search(text):
            errors.append(
                f"{javascript.relative_to(distribution)} contains a root-absolute generated URL"
            )
        if JS_NETWORK_URL.search(text):
            errors.append(
                f"{javascript.relative_to(distribution)} contains a runtime network URL"
            )

    # Everything in dist/ is uploaded, so every file is read: the WebAssembly
    # file is where a build records source paths, but nothing else may carry
    # one either. The account's name is noted and not failed on, since this
    # runs for every contributor and in CI, where that name is often a word.
    paths = machine_paths.examine([distribution])
    for note in paths.notes:
        print(f"note: {note}", file=sys.stderr)
    errors.extend(paths.errors)

    if errors:
        fail(errors)

    # A distribution that passes says nothing if nothing could have failed, so
    # the scripts are then run on what they must refuse.
    if prove:
        fail([f"the check cannot be trusted: {line}" for line in machine_paths.prove(distribution)])

    print(
        "Verified web distribution: "
        f"index.html, {len(root_js)} root JS, {len(root_wasm)} root WASM, "
        f"{len(output_wavs)} copied WAV files, relative local resources only, "
        f"no path of this machine. {paths.summary}"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "distribution",
        nargs="?",
        default="dist",
        type=Path,
        help="Trunk output directory (default: dist)",
    )
    parser.add_argument(
        "--without-proof",
        action="store_true",
        help="verify only: do not go on to run the scripts on what they must refuse "
        "(that run uses this itself)",
    )
    arguments = parser.parse_args()
    verify(arguments.distribution.resolve(), prove=not arguments.without_proof)


if __name__ == "__main__":
    main()
