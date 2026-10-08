#!/usr/bin/env bash
# Hourly cron: download the VdM eco-counter CSVs (one per year), keep only the
# catalogued counters, and atomically replace data/vdm-eco/<year>.csv.
#
# The current year is refreshed on every run; during January the previous
# year is refreshed too, to pick up late December uploads.  Older years are
# fetched once, and again whenever COUNTER_IDS changes (tracked by a stamp
# file), since each yearly upstream file is 70-90 MB.
#
# Install (user crontab on the server, like refresh-telraam.sh):
#
#     27 * * * * BIKESTAT_DATA_DIR=/var/www/bikestat/data \
#         /home/rhoge/GitHub/BikeStat/scripts/refresh-vdm-eco.sh \
#         >> /home/rhoge/bikestat-vdm-eco.log 2>&1
#
# For local dev, populate static/data/vdm-eco/ once with:
#
#     BIKESTAT_DATA_DIR=static/data ./scripts/refresh-vdm-eco.sh
set -euo pipefail

# Must match VDM_ECO_FIRST_YEAR and the id_compteur values in VDM_ECO_SITES
# (src/data/sources.rs).  Update in lock-step when adding a counter.
FIRST_YEAR=2024
COUNTER_IDS='100011783|100060991|100060992|100061090'

DATASET_API="https://donnees.montreal.ca/api/3/action/package_show?id=142ff2e9-7d0a-47d6-b4f6-dfeb97041daf"
UA="Mozilla/5.0 (BikeStat-cron)"

DATA_DIR="${BIKESTAT_DATA_DIR:-/var/www/bikestat/data}"
OUT_DIR="${DATA_DIR}/vdm-eco"
STAMP="${OUT_DIR}/.counter-ids"

mkdir -p "$OUT_DIR"

CUR_YEAR=$(TZ=America/Montreal date +%Y)
CUR_MONTH=$(TZ=America/Montreal date +%m)

IDS_CHANGED=0
[ "$(cat "$STAMP" 2>/dev/null || true)" = "$COUNTER_IDS" ] || IDS_CHANGED=1

# Each year's CSV lives under its own resource UUID, so look the URLs up in
# the dataset metadata rather than hardcoding them (new years appear there).
# donnees.montreal.ca rejects non-Mozilla user agents with 403.
PACKAGE_JSON=$(curl -fsSL --max-time 60 --retry 2 -A "$UA" "$DATASET_API")

FAIL_COUNT=0
for ((YEAR = FIRST_YEAR; YEAR <= CUR_YEAR; YEAR++)); do
    DEST="${OUT_DIR}/${YEAR}.csv"
    if [ "$YEAR" -ne "$CUR_YEAR" ] && [ -s "$DEST" ] && [ "$IDS_CHANGED" -eq 0 ] \
        && ! { [ "$YEAR" -eq $((CUR_YEAR - 1)) ] && [ "$CUR_MONTH" = "01" ]; }; then
        continue
    fi

    # Older years are named comptagevelo<year>.csv, newer comptage_velo_<year>.csv.
    URL=$(printf '%s' "$PACKAGE_JSON" \
        | grep -oE "https://[^\"]*/download/comptage_?velo_?${YEAR}[^\"/]*\.csv" \
        | head -1 || true)
    if [ -z "$URL" ]; then
        echo "refresh-vdm-eco: no resource found for ${YEAR}" >&2
        FAIL_COUNT=$((FAIL_COUNT + 1))
        continue
    fi

    RAW_TMP="${DEST}.raw.tmp"
    FILTERED_TMP="${DEST}.filtered.tmp"
    if ! curl -fsSL --max-time 300 --retry 2 -A "$UA" "$URL" -o "$RAW_TMP"; then
        rm -f "$RAW_TMP"
        echo "refresh-vdm-eco: download failed for ${YEAR}" >&2
        FAIL_COUNT=$((FAIL_COUNT + 1))
        continue
    fi

    # Catch HTML error pages and truncated bodies; keep the previous artifact.
    if ! head -1 "$RAW_TMP" | grep -q '^date,heure,id_compteur,nb_passages'; then
        rm -f "$RAW_TMP"
        echo "refresh-vdm-eco: unexpected content for ${YEAR}, keeping previous artifact" >&2
        FAIL_COUNT=$((FAIL_COUNT + 1))
        continue
    fi

    # A year with none of our counters legitimately yields a header-only file.
    { head -1 "$RAW_TMP"; grep -E ",(${COUNTER_IDS})," "$RAW_TMP" || true; } > "$FILTERED_TMP"
    rm -f "$RAW_TMP"
    mv -f "$FILTERED_TMP" "$DEST"
done

if [ "$FAIL_COUNT" -gt 0 ]; then
    exit 1
fi
printf '%s\n' "$COUNTER_IDS" > "$STAMP"
