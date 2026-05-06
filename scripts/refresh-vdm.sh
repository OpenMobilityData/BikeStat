#!/usr/bin/env bash
# Hourly cron job: download the VdM cyclistes CSV and atomically replace the
# served copy.  Also writes a short freshness indicator to status.txt so the
# UI can show "as of HH:MM".
#
# Install (example, adjust path to your deployment):
#
#   sudo install -m 0755 -o www-data -g www-data \
#     scripts/refresh-vdm.sh /opt/bikestat/refresh-vdm.sh
#   sudo tee /etc/cron.d/bikestat-vdm <<'EOF'
#   # Refresh BikeStat's VdM CSV at minute 7 every hour.
#   7 * * * * www-data /opt/bikestat/refresh-vdm.sh
#   EOF
#
# DATA_DIR should point at the served `data/` directory of the deployed app
# (i.e. dist/data after rsync, or wherever lighttpd serves /data/ from).
set -euo pipefail

URL="https://donnees.montreal.ca/dataset/142ff2e9-7d0a-47d6-b4f6-dfeb97041daf/resource/a8e463ab-d334-4714-81d5-8da0310d80c0/download/cyclistes.csv"

# Coarse street-name pre-filter.  Must include every location used by
# MONTREAL_LOCATION_FILTER in src/data/sources.rs; the WASM parser still
# applies the precise (rue_1, rue_2) match afterwards.  Update this regex
# in lock-step whenever a new VdM location is added to the catalogue.
FILTER_RE='bourret|girouard'

DATA_DIR="${BIKESTAT_DATA_DIR:-/var/www/bikestat/data}"
DEST="${DATA_DIR}/cyclistes.csv"
STATUS="${DATA_DIR}/status.txt"
RAW_TMP="${DEST}.raw.tmp"
FILTERED_TMP="${DEST}.filtered.tmp"
STATUS_TMP="${STATUS}.tmp"

mkdir -p "$DATA_DIR"

# Fetch with a generous timeout but fail loudly so the existing artifact
# survives if upstream is down or returns an HTTP error.
# Notes: donnees.montreal.ca rejects non-Mozilla user agents with 403, and
# serves the CSV via a redirect, so -L is required.
curl -fsSL --max-time 120 --retry 2 \
    -A "Mozilla/5.0 (BikeStat-cron)" \
    "$URL" -o "$RAW_TMP"

# Sanity check: first line should be the expected CSV header.  Catches HTML
# error pages and partial downloads — keep the previous artifact untouched.
if ! head -1 "$RAW_TMP" | grep -q '^agg_code,instance,longitude,'; then
    rm -f "$RAW_TMP"
    echo "refresh-vdm: unexpected content (not a VdM CSV), keeping previous artifact" >&2
    exit 1
fi

# Pre-filter: keep only rows whose street names match any catalogued
# location.  The full file is ~190 MB / 1.47M rows; the filtered file is
# ~10 MB / 70K rows.  Header line is always preserved.
{ head -1 "$RAW_TMP"; grep -iE "$FILTER_RE" "$RAW_TMP"; } > "$FILTERED_TMP"
rm -f "$RAW_TMP"

# Bail out if the filter produced nothing useful (header alone) — likely a
# regex mismatch after an upstream column rename or a network-truncated body.
if [ "$(wc -l < "$FILTERED_TMP")" -lt 2 ]; then
    rm -f "$FILTERED_TMP"
    echo "refresh-vdm: filtered output is empty, keeping previous artifact" >&2
    exit 1
fi

mv -f "$FILTERED_TMP" "$DEST"

# Status string: ISO 8601 UTC timestamp.  The client parses this and
# converts to the browser's local timezone for display, then prepends a
# localized "VdM data:" / "Données VdM:" prefix.
printf '%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" > "$STATUS_TMP"
mv -f "$STATUS_TMP" "$STATUS"
