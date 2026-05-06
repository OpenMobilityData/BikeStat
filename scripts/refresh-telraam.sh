#!/usr/bin/env bash
# Hourly cron: fetch the last N days of Telraam API data per configured
# segment and atomically replace the served JSON snapshots.  The client
# appends these to the historical xlsx-based records, so users see a
# seamless time series across the legacy and current sensors.
#
# Config file format (one segment per line, whitespace-separated):
#
#     # legacy_dir   api_segment_id   api_token
#     9794           9000007290       <token1>
#     10045          9000007489       <token2>
#
# Comments (`#`) and blank lines are ignored.  The config path is read
# from BIKESTAT_TELRAAM_CONFIG (default: ~/.bikestat-telraam.conf).
#
# Install:
#
#     chmod 600 ~/.bikestat-telraam.conf
#     crontab -e   # add a line like:
#     17 * * * * BIKESTAT_DATA_DIR=/var/www/bikestat/data \
#         /home/rhoge/GitHub/BikeStat/scripts/refresh-telraam.sh \
#         >> /home/rhoge/bikestat-telraam.log 2>&1

set -euo pipefail

CONFIG="${BIKESTAT_TELRAAM_CONFIG:-$HOME/.bikestat-telraam.conf}"
DATA_DIR="${BIKESTAT_DATA_DIR:-/var/www/bikestat/data}"
WINDOW_DAYS="${BIKESTAT_TELRAAM_WINDOW_DAYS:-90}"
ENDPOINT="https://telraam-api.net/v1/reports/traffic"

if [ ! -r "$CONFIG" ]; then
    echo "refresh-telraam: config file not found: $CONFIG" >&2
    exit 1
fi

# Telraam wants `YYYY-MM-DD HH:MM:SSZ` (space separator, Z suffix). Round
# down to the hour so multiple runs hit the same buckets and the response
# stays cacheable upstream.
END=$(date -u '+%Y-%m-%d %H:00:00Z')
START=$(date -u -d "${WINDOW_DAYS} days ago" '+%Y-%m-%d %H:00:00Z')

OK_COUNT=0
FAIL_COUNT=0

while IFS= read -r LINE || [ -n "$LINE" ]; do
    LINE="${LINE%%#*}"
    # shellcheck disable=SC2086
    set -- $LINE
    [ "$#" -eq 0 ] && continue
    if [ "$#" -ne 3 ]; then
        echo "refresh-telraam: skipping malformed line: $LINE" >&2
        FAIL_COUNT=$((FAIL_COUNT + 1))
        continue
    fi
    LEGACY_DIR="$1"
    API_SEG_ID="$2"
    API_TOKEN="$3"

    OUT_DIR="${DATA_DIR}/telraam/${LEGACY_DIR}"
    OUT_FILE="${OUT_DIR}/api.json"
    TMP_FILE="${OUT_FILE}.tmp"

    mkdir -p "$OUT_DIR"

    REQ_BODY=$(printf '{"level":"segments","id":"%s","format":"per-hour","time_start":"%s","time_end":"%s"}' \
        "$API_SEG_ID" "$START" "$END")

    HTTP_CODE=$(curl -sS --max-time 60 --retry 2 \
        -H "X-Api-Key: $API_TOKEN" \
        -H "Content-Type: application/json" \
        -d "$REQ_BODY" \
        -o "$TMP_FILE" \
        -w '%{http_code}' \
        "$ENDPOINT") || HTTP_CODE="000"

    if [ "$HTTP_CODE" != "200" ]; then
        rm -f "$TMP_FILE"
        echo "refresh-telraam: HTTP $HTTP_CODE for segment $API_SEG_ID (dir $LEGACY_DIR)" >&2
        FAIL_COUNT=$((FAIL_COUNT + 1))
        continue
    fi

    # Sanity check: the response must contain a "report" array.  Catches
    # silent error pages (e.g. an HTML 200 from a misconfigured proxy).
    if ! head -c 4096 "$TMP_FILE" | grep -q '"report"'; then
        rm -f "$TMP_FILE"
        echo "refresh-telraam: unexpected response shape for segment $API_SEG_ID" >&2
        FAIL_COUNT=$((FAIL_COUNT + 1))
        continue
    fi

    mv -f "$TMP_FILE" "$OUT_FILE"
    OK_COUNT=$((OK_COUNT + 1))
done < "$CONFIG"

# Fail loudly if every segment failed — keeps cron's "any output = problem"
# style alerting useful while letting partial successes pass quietly.
if [ "$OK_COUNT" -eq 0 ] && [ "$FAIL_COUNT" -gt 0 ]; then
    exit 1
fi
