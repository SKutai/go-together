#!/usr/bin/env python3
"""Build the native release executable without this machine's paths, and check it.

Runs `cargo build --locked --release` from the repository root with the
path-remapping flags of `scripts/machine_paths.py`, then fails if the
executable still holds a path of this machine. Further arguments are passed on
to Cargo, so `--target x86_64-apple-darwin` builds for another target the same
way. See `docs/DISTRIBUTION.md`.
"""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys

import machine_paths


def main() -> int:
    # The JSON messages are how Cargo says where it put things, which depends
    # on the target and on where the build directory is. Diagnostics and
    # progress still reach the terminal as usual.
    command = [
        "cargo",
        "build",
        "--locked",
        "--release",
        "--message-format=json-render-diagnostics",
        *sys.argv[1:],
    ]
    machine_paths.announce()
    executables: list[Path] = []
    written: list[Path] = []
    try:
        with subprocess.Popen(
            command,
            cwd=machine_paths.REPOSITORY,
            env=machine_paths.remapping_environment(),
            stdout=subprocess.PIPE,
            encoding="utf-8",
        ) as build:
            assert build.stdout is not None
            for line in build.stdout:
                try:
                    message = json.loads(line)
                except json.JSONDecodeError:
                    sys.stdout.write(line)
                    continue
                if message.get("reason") == "compiler-artifact" and message.get("executable"):
                    executables.append(Path(message["executable"]))
                    written.append(Path(message["executable"]))
                if message.get("reason") == "build-script-executed" and message.get("out_dir"):
                    written.append(Path(message["out_dir"]))
    except OSError as error:
        print(f"error: cannot run cargo: {error}", file=sys.stderr)
        return 1
    if build.returncode != 0:
        return build.returncode
    if not executables:
        print("error: Cargo reported no executable", file=sys.stderr)
        return 1

    # The flags were made from where Cargo said it would build, before it was
    # given the arguments above. One of them can move the build (`--target-dir`,
    # `--config`), and what was then written is recorded by its real path.
    known = [Path(folder) for folder in machine_paths.survey().build_directories]
    strays = [path for path in written if not any(machine_paths.under(path, folder) for folder in known)]
    if strays:
        print(
            f"error: Cargo wrote to {strays[0]}, which is not where it said it would build "
            "before it was given this command's arguments, so the flags did not cover it. Give "
            "a build directory in CARGO_TARGET_DIR or a configuration file, not on the command "
            "line.",
            file=sys.stderr,
        )
        return 1

    built = ", ".join(
        f"{machine_paths.path_search.shown(executable)} ({executable.stat().st_size} bytes)"
        for executable in executables
    )
    return machine_paths.tell(
        machine_paths.examine(executables), f"Built {built}, with no path of this machine."
    )


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        # Cargo has had the same signal and has said what it had to say.
        raise SystemExit(130)
