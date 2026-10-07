#!/usr/bin/env bash
# Build and deploy BikeStat to the production VPS.
#
# Excludes data/cyclistes.csv and data/status.txt so deploys never overwrite
# the cron-managed copies on the server with the local bootstrap files.
set -euo pipefail

REMOTE="${BIKESTAT_REMOTE:-rhoge@bikestat.org}"
DEST="${BIKESTAT_DEST:-/var/www/bikestat/}"

cd "$(dirname "$0")/.."

# Make sure cargo / trunk are on PATH when invoked from a non-login shell.
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# CARTO basemaps key is compiled into the wasm (see src/components/map.rs).
# Take it from the environment, else from the gitignored .carto-api-key file.
if [ -z "${CARTO_API_KEY:-}" ] && [ -f .carto-api-key ]; then
    CARTO_API_KEY="$(tr -d '[:space:]' < .carto-api-key)"
fi
if [ -z "${CARTO_API_KEY:-}" ]; then
    echo "error: CARTO_API_KEY not set and .carto-api-key missing" >&2
    exit 1
fi
export CARTO_API_KEY

trunk build --release

rsync -av --delete \
    --exclude='data/cyclistes.csv' \
    --exclude='data/cyclistes-all.csv' \
    --exclude='data/status.txt' \
    --exclude='data/telraam/*/api.json' \
    dist/ "${REMOTE}:${DEST}"
