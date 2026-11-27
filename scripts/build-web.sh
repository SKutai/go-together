#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd "$(dirname "$0")" && pwd)
REPOSITORY_ROOT=$(dirname "$SCRIPT_DIR")

cd "$REPOSITORY_ROOT"

if ! command -v trunk >/dev/null 2>&1; then
  echo "error: Trunk 0.21.14 is required (cargo install trunk --locked --version 0.21.14)" >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "error: python3 is required to verify the web distribution" >&2
  exit 1
fi

# wasm-release (Cargo.toml) is a size-tuned profile used only for this
# distributable build, not for `trunk serve` local development - passed as a
# CLI flag rather than set in Trunk.toml's [build] section so it does not
# also slow down every dev-mode `trunk serve` iteration. See Cargo.toml's
# comment on [profile.wasm-release] for the measurement behind it.
#
# The build runs through machine_paths.py, which adds rustc flags to the
# environment so that the WebAssembly file records no path of this machine;
# verify_web_build.py then fails if any file in dist/ holds one. The flags
# join whatever RUSTFLAGS already holds rather than replacing it.
python3 scripts/machine_paths.py run trunk build --release --cargo-profile wasm-release
python3 scripts/verify_web_build.py dist
