#!/usr/bin/env bash
# Build and deploy BikeStat to the production VPS.
#
# Excludes data/cyclistes.csv and data/status.txt so deploys never overwrite
# the cron-managed copies on the server with the local bootstrap files.
set -euo pipefail

REMOTE="${BIKESTAT_REMOTE:-rhoge@bikestat.org}"
DEST="${BIKESTAT_DEST:-/var/www/bikestat/}"

cd "$(dirname "$0")/.."

trunk build --release

rsync -av --delete \
    --exclude='data/cyclistes.csv' \
    --exclude='data/status.txt' \
    dist/ "${REMOTE}:${DEST}"
