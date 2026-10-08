#!/usr/bin/env bash
# Hourly cron job: rebuild the served VdM cyclistes CSV from the city's
# per-year files (cyclistes_<year>.csv) and atomically replace it.  Also
# writes a short freshness indicator to status.txt so the UI can show
# "as of HH:MM".
#
# The city stopped updating the single rolling cyclistes.csv on 2026-08-24
# and now publishes one file per year.  The current year is re-downloaded on
# every run (and the previous year during January, for late December
# uploads).  Filtered copies of past years are cached in CACHE_DIR, outside
# the web root so deploys don't delete them, and re-fetched only when
# FILTER_RE changes.
#
# Install (user crontab on the server):
#
#   7 * * * * BIKESTAT_DATA_DIR=/var/www/bikestat/data \
#       /home/rhoge/GitHub/BikeStat/scripts/refresh-vdm.sh \
#       >> /home/rhoge/bikestat-refresh.log 2>&1
#
# DATA_DIR should point at the served `data/` directory of the deployed app
# (i.e. dist/data after rsync, or wherever lighttpd serves /data/ from).
set -euo pipefail

# First year of per-year files to include.
FIRST_YEAR=2025

# Coarse street-name pre-filter.  Must include every location used by
# MONTREAL_LOCATION_FILTER in src/data/sources.rs; the WASM parser still
# applies the precise (rue_1, rue_2) match afterwards.  Update this regex
# in lock-step whenever a new VdM location is added to the catalogue.
FILTER_RE='bourret|girouard'

DATASET_API="https://donnees.montreal.ca/api/3/action/package_show?id=142ff2e9-7d0a-47d6-b4f6-dfeb97041daf"
UA="Mozilla/5.0 (BikeStat-cron)"

DATA_DIR="${BIKESTAT_DATA_DIR:-/var/www/bikestat/data}"
CACHE_DIR="${BIKESTAT_CACHE_DIR:-$HOME/.cache/bikestat/cyclistes}"
DEST="${DATA_DIR}/cyclistes.csv"
STATUS="${DATA_DIR}/status.txt"
DEST_TMP="${DEST}.tmp"
STATUS_TMP="${STATUS}.tmp"
STAMP="${CACHE_DIR}/.filter-re"

mkdir -p "$DATA_DIR" "$CACHE_DIR"

CUR_YEAR=$(TZ=America/Montreal date +%Y)
CUR_MONTH=$(TZ=America/Montreal date +%m)

FILTER_CHANGED=0
[ "$(cat "$STAMP" 2>/dev/null || true)" = "$FILTER_RE" ] || FILTER_CHANGED=1

# Each year's CSV lives under its own resource UUID, so look the URLs up in
# the dataset metadata rather than hardcoding them (new years appear there).
# donnees.montreal.ca rejects non-Mozilla user agents with 403, and serves
# downloads via a redirect, so -L is required.
PACKAGE_JSON=$(curl -fsSL --max-time 60 --retry 2 -A "$UA" "$DATASET_API")

for ((YEAR = FIRST_YEAR; YEAR <= CUR_YEAR; YEAR++)); do
    CACHED="${CACHE_DIR}/${YEAR}.csv"
    if [ "$YEAR" -ne "$CUR_YEAR" ] && [ -s "$CACHED" ] && [ "$FILTER_CHANGED" -eq 0 ] \
        && ! { [ "$YEAR" -eq $((CUR_YEAR - 1)) ] && [ "$CUR_MONTH" = "01" ]; }; then
        continue
    fi

    URL=$(printf '%s' "$PACKAGE_JSON" \
        | grep -oE "https://[^\"]*/download/cyclistes_${YEAR}\.csv" \
        | head -1 || true)
    if [ -z "$URL" ]; then
        # A new year's file may not be published yet in early January.
        echo "refresh-vdm: no resource found for ${YEAR}, skipping" >&2
        continue
    fi

    RAW_TMP="${CACHED}.raw.tmp"
    FILTERED_TMP="${CACHED}.filtered.tmp"
    # Fail loudly so the existing artifact survives if upstream is down.
    curl -fsSL --max-time 300 --retry 2 -A "$UA" "$URL" -o "$RAW_TMP"

    # Sanity check: first line should be the expected CSV header.  Catches
    # HTML error pages and partial downloads — keep the previous artifact.
    if ! head -1 "$RAW_TMP" | grep -q '^agg_code,instance,longitude,'; then
        rm -f "$RAW_TMP"
        echo "refresh-vdm: unexpected content for ${YEAR} (not a VdM CSV), keeping previous artifact" >&2
        exit 1
    fi

    # Pre-filter: keep only hourly ("h") and daily ("d") rows — the only
    # agg_codes the WASM parser reads — whose street names match a catalogued
    # location.  A full year is ~200 MB; the filtered file is ~1-4 MB.
    { head -1 "$RAW_TMP"
      grep -E '^"?[hd]"?,' "$RAW_TMP" | grep -iE "$FILTER_RE" || true
    } > "$FILTERED_TMP"
    rm -f "$RAW_TMP"

    # The current year must yield rows; an empty result likely means a regex
    # mismatch after an upstream column rename or a truncated body.
    if [ "$YEAR" -eq "$CUR_YEAR" ] && [ "$(wc -l < "$FILTERED_TMP")" -lt 2 ]; then
        rm -f "$FILTERED_TMP"
        echo "refresh-vdm: filtered output for ${YEAR} is empty, keeping previous artifact" >&2
        exit 1
    fi
    mv -f "$FILTERED_TMP" "$CACHED"
done
printf '%s\n' "$FILTER_RE" > "$STAMP"

# Concatenate all years under a single header.
FIRST=1
: > "$DEST_TMP"
for ((YEAR = FIRST_YEAR; YEAR <= CUR_YEAR; YEAR++)); do
    CACHED="${CACHE_DIR}/${YEAR}.csv"
    [ -s "$CACHED" ] || continue
    if [ "$FIRST" -eq 1 ]; then
        cat "$CACHED" >> "$DEST_TMP"
        FIRST=0
    else
        tail -n +2 "$CACHED" >> "$DEST_TMP"
    fi
done
if [ "$(wc -l < "$DEST_TMP")" -lt 2 ]; then
    rm -f "$DEST_TMP"
    echo "refresh-vdm: combined output is empty, keeping previous artifact" >&2
    exit 1
fi
mv -f "$DEST_TMP" "$DEST"

# Status string: ISO 8601 UTC timestamp.  The client parses this and
# converts to the browser's local timezone for display, then prepends a
# localized "VdM data:" / "Données VdM:" prefix.
printf '%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" > "$STATUS_TMP"
mv -f "$STATUS_TMP" "$STATUS"
