# BikeStat

Web application for visualizing traffic-count data from a curated set of
sensors in Côte-des-Neiges–Notre-Dame-de-Grâce, Montréal. The catalogue is
intentionally narrow so observations from different sensor networks can be
overlaid on the same chart for direct comparison; additional locations and
network types can be registered without code changes outside the source
catalogue.

Production deployment: https://bikestat.org

## Scope

- Aggregates bike-count data across multiple source networks (currently
  Ville de Montréal, Telraam, and Eco-Counter via borough
  access-to-information requests).
- Supports auxiliary modalities (pedestrians, cars, trucks) when a source
  provides them; bicycles are the primary focus.
- Renders a time series with Hour / Day / Week / Month bucket resolution
  and a Year-on-Year comparison mode that folds successive 12-month
  windows onto a shared axis.
- Allows arbitrary combinations of sensors, directions, and modalities
  to be overlaid on the same axes.
- Bilingual interface (English / French), browser-detected and toggled
  in-page; preference is persisted in `localStorage`.

## Data sources

| Source | Loader | Cadence |
|---|---|---|
| Ville de Montréal `cyclistes.csv` | Server cron pre-filters to catalogued streets and serves the result statically | Hourly |
| Telraam (legacy S1 + current S2 sensors) | Historical xlsx exports plus a rolling 90-day JSON snapshot fetched from the Level-5 API by a server cron | Hourly |
| CDN-NDG borough eco-counter | Quarterly xlsx batches obtained via access-to-information requests, loaded statically | Manual on receipt |

API credentials live only on the server (in `~/.bikestat-telraam.conf`)
and are never embedded in the WASM bundle.

## Stack

- Rust + Leptos 0.7 client-side rendering, compiled to WebAssembly via
  Trunk.
- All aggregation, charting, and i18n run in the browser; the server
  hosts only static files (lighttpd) plus the cron jobs that refresh the
  live data feeds.
- TLS via Let's Encrypt with auto-renewal.

## Repository layout

```
src/
  lib.rs              App root; signal wiring; data-load orchestration
  i18n.rs             EN / FR translation table
  data/
    types.rs          Domain types (DataSource, CountRecord, Modality, …)
    sources.rs        Catalogue of registered sensors / locations
    loader.rs         Per-source parsers and fetch helpers
  components/
    chart.rs          SVG line/area chart with hover crosshair + tooltip
    map.rs            HTML/CSS overview map
    sidebar.rs        Filter panel (locations, modalities, date presets)
scripts/
  deploy.sh           trunk build + rsync (excludes cron-managed files)
  refresh-vdm.sh      Hourly cron: VdM CSV download + street filter
  refresh-telraam.sh  Hourly cron: Telraam API JSON snapshot per segment
static/
  style.css
  favicon.svg
  data/               Static data files (xlsx exports, etc.)
Notes/                Session handoff documents (deploy playbook, etc.)
```

## Local development

```
trunk serve
```

Trunk compiles the WASM bundle and reloads on source changes. The
Telraam and CDN-NDG xlsx files load from `static/data/`; the VdM CSV and
the Telraam API JSON snapshots are populated only on the production
server, so those feeds show as unavailable in dev unless a copy is
dropped in by hand for testing.

## Deployment

```
./scripts/deploy.sh
```

Runs `trunk build --release` and rsyncs `dist/` to the production host,
excluding `data/cyclistes.csv`, `data/status.txt`, and
`data/telraam/*/api.json` so the cron-managed files on the server are
not overwritten.

The server's lighttpd vhost, certbot configuration, and cron entries
are documented in
`Notes/2026-05-06-02-44-53-Server-Deploy-Handoff.md`.

## Adding sources or locations

- **Ville de Montréal street**: add to `MONTREAL_LOCATION_FILTER` in
  `src/data/sources.rs` *and* to the `FILTER_RE` regex in
  `scripts/refresh-vdm.sh` (the shell filter must remain a superset).
- **Telraam segment**: register via `push_telraam_segment` in
  `sources.rs`, drop the historical xlsx exports under
  `static/data/telraam/<segment>/`, and add the segment ID + API token
  to `~/.bikestat-telraam.conf` on the server for ongoing API coverage.
- **CDN-NDG eco-counter batch**: drop the xlsx under
  `static/data/cdn-ndg/<location>/<period>.xlsx` and append the path to
  `cdn_ndg_sources()` in `sources.rs`.
