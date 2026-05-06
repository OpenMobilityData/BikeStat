# BikeStat — Post-Deploy UX Iteration Handoff
**Date / Time (Montreal):** 2026-05-06 07:24 EDT
**Session focus:** Post-deploy refinement after the initial production launch.
**Live site:** https://bikestat.org (HTTPS via Let's Encrypt, hourly VdM cron, deployed)

---

## 1. Current task & objective

BikeStat — a Rust/WebAssembly client-side web app (Leptos 0.7 CSR) that
aggregates and visualizes traffic-count data for a small, curated set of
locations in Côte-des-Neiges–Notre-Dame-de-Grâce, Montréal. The app is
**live in production** at https://bikestat.org served by lighttpd from a
small Ubuntu 24.04 VPS. Repo: `git@github.com:OpenMobilityData/BikeStat.git`
(public).

This session was a sequence of small UX refinements, deployed live in real
time using the Mac-build → `rsync dist/` → `git pull` server loop. Each
change was committed individually and tested against the live site as a
deploy-pipeline exercise.

The companion document `Notes/2026-05-06-Server-Deploy-Handoff.md` is the
as-built deploy playbook (lighttpd vhost, certbot, cron, etc.). Read that
first if you need to reproduce the server setup; this doc covers what
changed *after* the site went live.

---

## 2. Progress completed (this session)

In commit order (most recent first):

| Commit  | What it did |
|---------|-------------|
| `61b8a82` | Tooltip flip near viewport edges — applies CSS `translate(-100% - 14px, …)` when the cursor is within ~300 px of the right edge or ~200 px of the bottom, so the floating chart tooltip never overflows the viewport. |
| `0acb565` | Replaced the emoji favicon with a vector mini bar chart (red / blue / green ascending bars on dark navy). Pure vector shapes — emoji-as-`<text>` doesn't reliably rasterize in browser favicon contexts. |
| `b50ef69` | Hover snap switched from nearest-by-time to **Euclidean distance in SVG space**. At hour resolution, several hours can pack into one pixel, so a peak's adjacent low-value hour was winning the time-only race. Euclidean means moving the cursor up toward a tall spike snaps onto it, which matches user intuition. |
| `fbac80d` | Tooltip date formatting now resolution-aware: Hour → Montreal local time with `%Z` abbreviation (e.g. `2025-09-09 04:00 EDT`); Day / Week / Month → date only (`2025-09-09`). Required passing `resolution: ReadSignal<Resolution>` to `Chart`. |
| `dca7602` | Added the hover crosshair + per-series tooltip itself. Transparent SVG `<rect>` over the plot area captures `mousemove`/`mouseleave`; vertical dashed crosshair line + per-series colored dots render inside the SVG; floating HTML tooltip rendered with `position: fixed` shows the date and a row per series with color swatch + label + numeric value. Pulled in shared `ChartGeom` struct so rendering and hover use the same coordinate transform. |
| `ce6bb8a` | Expanded the empty-chart placeholder to "Please select one or more locations to view counts". |
| `4b6b289` | Earlier reword of the same placeholder; superseded by the line above. |
| `65e3b76` | Removed redundant ` (NDG)` suffix from Telraam display names. (And, in the same commit, set the compass labels — `dir_a_to_b` / `dir_b_to_a` — for segments 9794 and 10045.) |
| `acc9dfd` | Two changes bundled: (a) Disable preset buttons whose date span is too short for the current resolution (Week needs ≥ 14 days, Month needs ≥ 60 days; Hour/Day always enabled). The preset tuple grew a 4th `i64` field for span-in-days so the sidebar can compare without re-parsing. (b) Renamed the "Presets" sidebar heading to "Date range". |

Earlier in the day (separate session, already documented in
`Server-Deploy-Handoff.md`): initial deploy, lighttpd + HTTPS + cron all
wired up, public URL serving traffic.

---

## 3. Key decisions & patterns

### Deploy iteration loop (well-established)

The standing recipe for any change:

```bash
cd ~/Desktop/BikeStat
git add -A
git commit -m "<message>"
git push
trunk build --release
rsync -av --delete dist/ rhoge@bikestat.org:/var/www/bikestat/
ssh rhoge@bikestat.org 'cd ~/GitHub/BikeStat && git pull'
```

The server `git pull` keeps the cron script (`scripts/refresh-vdm.sh`) in
sync; if a change doesn't touch that file, the `git pull` step is
optional. The build runs on the developer's Mac — the VPS is too small
to compile Rust without swap-thrashing.

### Chart hover architecture

The chart's hover layer is built on three pieces:

1. **`ChartGeom`** (in `src/components/chart.rs`) — a small struct holding
   `x_min` / `y_min` / `x_span` / `y_span` plus the SVG paddings. Exposed
   `to_x` / `to_y` methods. Both the rendering closure and the
   `on:mousemove` handler call `compute_geom(...)` so they agree on
   coordinates.
2. **Transparent SVG `<rect>` overlay** at the plot area, drawn after the
   chart contents. CSS class `.chart-hover-area`, `cursor: crosshair`.
   Captures pointer events; subsequent `<g class="chart-hover">` (drawn
   on top) has `pointer-events: none` so the dots don't intercept.
3. **`HoverInfo`** signal (`Option<HoverInfo>`) — set on every mousemove,
   cleared on mouseleave. Holds: `crosshair_x` (SVG), `client_x/y`
   (viewport pixels), `flip_x/flip_y` (booleans for tooltip side), and a
   `Vec<HoverRow>` of per-series readouts.

Snap algorithm is **global Euclidean** — find the single nearest point
across all series in SVG space, then look up each series' value at that
anchor timestamp by nearest-in-time. Series share a Resolution grid so
the per-series time lookups typically hit the same timestamp exactly.

Tooltip uses `position: fixed` (so it can flow over any container) and
flips via CSS `transform` based on `flip_x` / `flip_y` flags computed in
the handler from `web_sys::window().inner_width()` /
`inner_height()`.

### Date-preset disable rule

Per-resolution minimum span (in `src/components/sidebar.rs`):

```rust
let min_days: i64 = match res {
    Resolution::Hour | Resolution::Day => 0,
    Resolution::Week  => 14,
    Resolution::Month => 60,
};
```

Each preset carries its day-span in the 4th tuple field. Buttons render
with the `disabled` attribute and a tooltip explaining why when
`days < min_days`. CSS `button:disabled { opacity: 0.4; cursor: not-allowed; }`.

### Color scheme

**Hue = location, lightness = modality** — inverted from the original on
user feedback because overlay comparison across sources turned out to be
the more common workflow. Locations cycle through 8 hand-picked HSL hues;
modality varies lightness within that hue (Trucks darkest → Motorcycles
lightest). Dash pattern is the primary modality cue; lightness is a
tiebreaker. (See `src/lib.rs::series_color`.)

### Preset list shape

`Vec<(String, String, String, i64)>` = `(label, from, to, days)`. The
`compute_date_presets` helper emits in order: "All dates", relatives
(Last Week / Month / 3 Months / 6 Months), calendar years, seasonal
(Summer / Winter). Each entry gates on overlap with the available data
window.

### Year-on-Year mode

Implemented as a separate `ViewMode::YearOnYear` toggle (not just a
preset). Anchors a 12-month axis at the earliest record matching the
current selection; every later 12-month block folds onto that axis with
a distinct hue per year-bucket. `update_date_range` is suppressed while
in YoY mode so late-arriving records don't shift the axis.

### Server-served VdM CSV with hourly refresh

`scripts/refresh-vdm.sh` runs hourly via the deploy user's crontab,
fetches the upstream CSV (with a Mozilla User-Agent — the city's portal
403s otherwise), filters down to rows matching `bourret|girouard`
(reduces 192 MB → 11 MB raw / ~316 KB gzipped), atomically replaces
`/var/www/bikestat/data/cyclistes.csv`, and writes a freshness string
to `/var/www/bikestat/data/status.txt` (read by the UI's "VdM data: …"
indicator).

---

## 4. Active files & locations

```
BikeStat/
├── Cargo.toml                          web-sys features include Element,
│                                       DomRect, MouseEvent (added for hover)
├── index.html                          title "BikeStat — Traffic Count Aggregator"
├── Notes/
│   ├── 2026-05-05-19-37-00-Session-1-Handoff.md       early dev (NoBikes era)
│   ├── 2026-05-05-21-45-26-Session-2-Handoff.md
│   ├── 2026-05-05-23-09-11-Session-3-Handoff.md
│   ├── 2026-05-06-Server-Deploy-Handoff.md            ★ as-built deploy playbook
│   └── 2026-05-06-07-24-48-Post-Deploy-UX-Iteration-Handoff.md   ← this file
├── scripts/
│   └── refresh-vdm.sh                  cron-invoked: filtered VdM refresh +
│                                       atomic rename + status.txt writer
├── src/
│   ├── lib.rs                          App root; signal wiring; series_color
│                                       (hue=loc, lightness=modality);
│                                       compute_date_presets; YoY logic;
│                                       update_date_range
│   ├── data/
│   │   ├── mod.rs
│   │   ├── types.rs                    CountRecord, DataSource (with `group`),
│   │                                   Modality, Resolution, ViewMode,
│   │                                   LoaderType (incl. CdnNdgExcel variant)
│   │   ├── sources.rs                  TELRAAM_ANNOTATIONS (compass labels
│   │                                   filled in for 9794 & 10045);
│   │                                   MONTREAL_LOCATION_FILTER;
│   │                                   cdn_ndg_sources(); push_*_segment helpers
│   │   └── loader.rs                   parse_montreal_cyclistes_csv (incl.
│   │                                   VdM: prefix + Total synthesis);
│   │                                   parse_telraam_excel; parse_cdn_ndg_excel;
│   │                                   aggregate (with full-bucket trimming)
│   └── components/
│       ├── mod.rs
│       ├── chart.rs                    SVG chart + crosshair tooltip overlay;
│       │                               ChartGeom + compute_geom shared between
│       │                               render and hover; format_hover_date
│       │                               (Montreal time at Hour resolution);
│       │                               Euclidean snap; viewport-edge flip
│       ├── map.rs                      HTML/CSS map; CartoDB dark_nolabels @2x
│       │                               tiles; integer tile zoom + CSS scale;
│       │                               edge-aware label flipping
│       └── sidebar.rs                  cluster grouping; date-range presets
│                                       with resolution-based disable;
│                                       YearOnYear button
└── static/
    ├── style.css                       Includes hover layer styles
    │                                   (.chart-crosshair, .chart-hover-dot,
    │                                   .chart-tooltip, .chart-tooltip-row,
    │                                   .chart-tooltip-swatch, etc.) and
    │                                   button:disabled rule
    ├── favicon.svg                     Vector mini bar chart (red/blue/green)
    └── data/
        ├── cyclistes.csv               (gitignored) populated locally for dev
        ├── status.txt                  (gitignored) populated by cron only
        ├── telraam/{9794,10045}/{2024,2025,2026}.xlsx
        └── cdn-ndg/terrebonne-kensington/2025-07-26_2025-11-15.xlsx
```

---

## 5. Current state

- **Working tree clean.** Local main is in sync with `origin/main` after
  the favicon and tooltip-flip commits.
- **Production live** at https://bikestat.org with valid Let's Encrypt
  cert. Apex + www both work, www → apex over HTTPS, HTTP → HTTPS for
  everything except `/.well-known/acme-challenge/` (so renewals can
  still complete over plain HTTP).
- **All session changes deployed and verified** by the user. The chart
  hover crosshair, tooltip, edge-flip, and resolution-aware date format
  are all functional in production.
- **Cron job hourly:** verified during initial deploy; the `VdM data: …`
  indicator in the header shows the most recent successful run.
- **Cert auto-renewal:** verified by `certbot renew --dry-run`. The
  deploy hook at `/etc/letsencrypt/renewal-hooks/deploy/lighttpd-bikestat.sh`
  rebuilds the combined PEM and reloads lighttpd.

No partially-completed work. Stopping point is clean.

---

## 6. Next steps

In approximate priority order:

### A. Possible polish (small, can ship one-by-one)

1. **Hide tooltip when far from any series line.** Currently the
   Euclidean snap always returns the closest point even if the cursor
   is in empty space. A threshold (e.g. > 80 SVG units from the nearest
   point) could suppress the tooltip in those regions, reducing noise.
2. **Highlight the row whose series is closest in y to the cursor.** Adds
   bold or background tint to one row in the multi-series tooltip to
   help identify which series the user is "really" pointing at.
3. **Optimize hover lookup with a binary search by time** for the Euclidean
   pass — current implementation is O(N×M) per mousemove. Fine at
   ~13 K total points; would matter if data scales 5–10×. Could limit
   the Euclidean scan to a `±k` window of timestamps around the cursor's
   time.
4. **More robust tooltip date for Hour resolution.** Right now the same
   bucket can appear as "2025-09-09 04:00 EDT" in the tooltip but
   "Sep 09" on the x-axis (UTC days). Not strictly wrong, but a tiny
   inconsistency.

### B. Data sources (open from way back)

5. **Telraam API integration** (`fetch_telraam_api(segment_id, key, from, to)`
   in `loader.rs`). Endpoint: `https://telraam-api.net/level5/reports/traffic`.
   Needed for ongoing data after the manually-loaded xlsx files run dry.
   API key handling not yet decided.
6. **Adding new VdM locations:** update both `MONTREAL_LOCATION_FILTER` in
   `src/data/sources.rs` *and* `FILTER_RE` in `scripts/refresh-vdm.sh`.
   The shell filter must remain a superset; otherwise the parser sees
   nothing.

### C. Operations

7. **Monitoring / analytics.** Currently zero. lighttpd's `mod_accesslog`
   writes basic access logs by default; if anyone else is testing,
   skim those occasionally. A small JS pixel later if you want
   structured hit counts.
8. **Add new ATI batches** as they arrive: drop the xlsx in
   `static/data/cdn-ndg/<location>/<YYYY-MM-DD_YYYY-MM-DD>.xlsx`,
   append the path to `cdn_ndg_sources()` in `sources.rs`, commit,
   push, redeploy.

### D. Architectural (deferred — not needed yet)

9. **Workspace split + ingest binary.** The architecture sketched
   earlier (one `bikestat-data` lib, one `bikestat-web` cdylib, one
   `bikestat-ingest` native binary) becomes worthwhile if traffic
   grows or if more sources need pre-processing. Not needed at current
   scale; revisit if the WASM bundle becomes a bottleneck.

---

## 7. Important context (gotchas, deps, etc.)

### Deploy iteration gotchas

- `dist/data/cyclistes.csv` and `dist/data/status.txt` are gitignored
  but Trunk's `copy-dir` on `static/data/` does include them in
  `dist/`. After `rsync --delete`, the bootstrap CSV (built locally)
  briefly replaces the cron-managed one on the server until the next
  cron tick (≤ 1 hour) refreshes it. To restore immediately, SSH and
  run `~/GitHub/BikeStat/scripts/refresh-vdm.sh` with
  `BIKESTAT_DATA_DIR=/var/www/bikestat/data`.
- Browsers cache favicons aggressively. After changing `static/favicon.svg`,
  expect users to need a hard reload (Cmd-Shift-R) or new tab to see
  the new icon. Trunk fingerprints the filename, which usually
  busts the cache, but Safari especially can hold the old one.

### Chart code gotchas

- `web-sys` features added for the hover layer: `Element`, `DomRect`,
  `MouseEvent`. Without these, `Element::get_bounding_client_rect`
  doesn't compile.
- The SVG uses `preserveAspectRatio="none"`. It's the intended choice
  for the chart so it stretches into its container, **but it means
  emoji-as-`<text>` and any ratio-preserving content do weird things**.
  The map was migrated to HTML/CSS for the same reason. Keep this in
  mind if introducing new SVG content with text or images.
- Chart hover snap is **Euclidean in SVG units** (not screen pixels).
  Since `preserveAspectRatio="none"` stretches the SVG, the visual
  distance per SVG unit differs in x and y. The snap still feels
  natural because both axes scale together with container size, but
  if you ever switch to `xMidYMid meet` you'll need to convert to
  rendered pixels for snap.

### Known minor inconsistency

- The chart's x-axis tick labels (`%b %d`) show UTC days; the tooltip
  date for Hour resolution shows Montreal local time with `%Z`. A
  bucket at 02:00 UTC (= 22:00 EDT the previous day) appears as
  "Sep 09" on the x-axis but "2025-09-08 22:00 EDT" in the tooltip.
  Acceptable for now; full fix would be bucketing by Montreal local
  date (bigger change in `bucket_key`).

### Server-side gotchas (recap)

- `mod_redirect` and `mod_openssl` are *not* exposed via
  `lighty-enable-mod` on Ubuntu 24.04. They're loaded directly via
  `server.modules += ( "mod_redirect" )` etc. at the top of our
  vhost config. `mod_deflate` is normal `lighty-enable-mod deflate`.
- The global `20-deflate.conf` ships with a 4-entry `deflate.mimetypes`
  list. Per-vhost overrides do not merge cleanly on lighttpd 1.4.74,
  so we extended the global list to include `text/csv`,
  `application/wasm`, `application/json`, `image/svg+xml`.
- The cron runs as the deploy user (`rhoge`) via user crontab; the
  data directory is owned by that same user. Avoids the
  `www-data`-vs-`rhoge` permission gymnastics.

### References to read next

- `Notes/2026-05-06-Server-Deploy-Handoff.md` — full deploy playbook,
  including DNS / certbot / cron / lighttpd config.
- Project memory at `~/.claude/projects/-Users-rhoge-Desktop-BikeStat/memory/`
  — covers stack, file layout, conventions. Loaded automatically by
  Claude Code.

### Recent commit history (top of `git log --oneline`)

```
61b8a82 Flip hover tooltip toward viewport interior near right/bottom edges
0acb565 Replace emoji favicon with vector mini bar chart
b50ef69 Use Euclidean distance for chart hover snap so peaks are reachable
fbac80d Format tooltip date by resolution; show Montreal time at Hour resolution
dca7602 Add hover crosshair + tooltip showing per-series values at cursor
ce6bb8a Expand empty-chart placeholder text
4b6b289 Reword empty chart state to match the Locations sidebar
65e3b76 Drop redundant (NDG) suffix from Telraam location labels
acc9dfd Disable too-short date presets at coarse resolutions; rename Presets → Date range
6b36590 Rewrite deploy handoff as as-built playbook
54e8d9d Add server deploy handoff document
9cc7234 Serve VdM CSV same-origin via hourly cron with location pre-filter
```
