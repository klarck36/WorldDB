#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
if [ -d "$cargo_bin" ]; then
    PATH="$cargo_bin:$PATH"
    export PATH
fi

WORLDDB_VERIFY_PROFILE=${WORLDDB_VERIFY_PROFILE:-dev}
if [ -z "${WORLDDB_PYTHON:-}" ]; then
    if command -v python3 >/dev/null 2>&1 && python3 --version >/dev/null 2>&1; then
        WORLDDB_PYTHON=python3
    elif command -v python >/dev/null 2>&1 && python --version >/dev/null 2>&1; then
        WORLDDB_PYTHON=python
    else
        WORLDDB_PYTHON=python3
    fi
fi
export WORLDDB_VERIFY_PROFILE WORLDDB_PYTHON

if [ -z "${CARGO_TARGET_DIR:-}" ]; then
    cache_root=${XDG_CACHE_HOME:-"$HOME/.cache"}
    CARGO_TARGET_DIR="$cache_root/worlddb/verify-target"
    export CARGO_TARGET_DIR
fi

exec cargo xtask verify "$@"
