#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

WORLDDB_VERIFY_PROFILE=${WORLDDB_VERIFY_PROFILE:-dev}
WORLDDB_PYTHON=${WORLDDB_PYTHON:-python3}
export WORLDDB_VERIFY_PROFILE WORLDDB_PYTHON

if [ -z "${CARGO_TARGET_DIR:-}" ]; then
    cache_root=${XDG_CACHE_HOME:-"$HOME/.cache"}
    CARGO_TARGET_DIR="$cache_root/worlddb/verify-target"
    export CARGO_TARGET_DIR
fi

exec cargo xtask verify "$@"
