mod data;
mod components;
mod i18n;

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, Months, NaiveDate, TimeZone, Utc};
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

use data::{loader, sources};
use data::sources::telraam_annotation;
use data::types::{CountRecord, DataSource, LoaderType, Modality, Resolution, ViewMode};
use components::chart::{Chart, Series};
use components::map::SourceMap;
use components::sidebar::Sidebar;
use i18n::Lang;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let (app_sources, set_sources) = signal::<Vec<DataSource>>(vec![]);
    let (records,     set_records) = signal::<Vec<CountRecord>>(vec![]);
    let (load_msgs,   set_load_msgs) = signal::<Vec<String>>(vec![]);
    // Per-segment last successful Telraam API refresh (api.json mtime,
    // captured from the `Last-Modified` response header on each fetch).
    // Distinct from the latest record timestamp — the cron may have run
    // moments ago even though the underlying sensor stopped reporting days
    // ago. Keyed by Telraam source id ("telraam-9794" etc.).
    let (telraam_api_fetched, set_telraam_api_fetched) =
        signal::<BTreeMap<String, DateTime<Utc>>>(BTreeMap::new());

    let (selected_srcs, set_selected_srcs) = signal::<Vec<String>>(vec![]);
    let (selected_mods, set_selected_mods) = signal::<Vec<Modality>>(vec![Modality::Bikes]);
    let (resolution,    set_resolution)    = signal(Resolution::Day);
    let (view_mode,     set_view_mode)     = signal(ViewMode::Linear);

    let now = Utc::now();
    let (date_from, set_date_from) = signal(
        NaiveDate::from_ymd_opt(now.year(), 1, 1).expect("Jan 1 is always valid")
    );
    let (date_to, set_date_to) = signal(
        NaiveDate::from_ymd_opt(now.year(), 12, 31).expect("Dec 31 is always valid")
    );

    // Filter panel toggle for narrow viewports.  CSS hides the sidebar by
    // default below ~768px and reveals it when this is true.  On wider
    // viewports the CSS rule has no effect — sidebar is always visible.
    let (sidebar_open, set_sidebar_open) = signal(false);

    // UI language: persisted across visits in localStorage, defaults to the
    // browser's preferred language. Provided as context so any component
    // can read it via `use_context::<ReadSignal<Lang>>()`.
    let (lang, set_lang) = signal(Lang::from_browser());
    Effect::new(move |_| lang.get().store());
    provide_context(lang);

    // ── Seed catalogue with pre-configured sources immediately ──
    // Their records load asynchronously below; the entries appear in the
    // sidebar right away so the user can see what is expected.
    let telraam = sources::telraam_sources();
    let cdn_ndg = sources::cdn_ndg_sources();
    set_sources.update(|s| {
        s.extend(telraam.clone());
        s.extend(cdn_ndg.clone());
    });

    // ── Data freshness indicator ──
    // Server cron writes a short string (e.g. "VdM data: 2026-05-06 14:00 EDT")
    // to data/status.txt after each successful refresh.  Some web servers
    // (including `trunk serve` and lighttpd in SPA-fallback mode) return
    // index.html with a 200 status when the requested file is missing, so
    // validate the response shape before trusting it.
    let (data_status, set_data_status) = signal::<Option<String>>(None);
    spawn_local(async move {
        if let Ok(resp) = gloo_net::http::Request::get("data/status.txt").send().await {
            if resp.ok() {
                if let Ok(text) = resp.text().await {
                    let trimmed = text.trim();
                    if !trimmed.is_empty()
                        && trimmed.len() < 200
                        && !trimmed.starts_with('<')
                    {
                        set_data_status.set(Some(trimmed.to_string()));
                    }
                }
            }
        }
    });

    // ── Fetch Montreal data ──
    {
        let set_sources   = set_sources.clone();
        let set_records   = set_records.clone();
        let set_load_msgs = set_load_msgs.clone();
        spawn_local(async move {
            add_msg(&set_load_msgs, "⏳ Loading Montréal data…");
            match loader::fetch_montreal_cyclistes().await {
                Ok((new_srcs, new_recs)) => {
                    update_date_range(&new_recs, view_mode, date_from, date_to, set_date_from, set_date_to);
                    set_sources.update(|s| s.extend(new_srcs));
                    set_records.update(|r| r.extend(new_recs));
                    remove_msg(&set_load_msgs, "⏳ Loading Montréal data…");
                }
                Err(e) => {
                    replace_msg(&set_load_msgs,
                        "⏳ Loading Montréal data…",
                        &format!("⚠ Montréal: {}", e));
                }
            }
        });
    }

    // ── Fetch each Telraam Excel file ──
    for src in &telraam {
        if let LoaderType::TelraamExcel { file_urls, .. } = &src.loader_type {
            for url in file_urls {
                let src_id    = src.id.clone();
                let url       = url.clone();
                let src_name  = src.name.clone();
                let set_records   = set_records.clone();
                let set_load_msgs = set_load_msgs.clone();
                spawn_local(async move {
                    let msg = format!("⏳ Loading {}…", src_name);
                    add_msg(&set_load_msgs, &msg);
                    match loader::fetch_telraam_excel(&src_id, &url).await {
                        Ok(new_recs) => {
                            update_date_range(&new_recs, view_mode, date_from, date_to, set_date_from, set_date_to);
                            set_records.update(|r| dedup_extend(r, new_recs));
                            remove_msg(&set_load_msgs, &msg);
                        }
                        Err(e) => {
                            replace_msg(&set_load_msgs, &msg,
                                &format!("⚠ {}: {}", src_name, e));
                        }
                    }
                });
            }
        }
    }

    // ── Fetch each Telraam API JSON snapshot (written by hourly server cron) ──
    // 404 means cron hasn't produced one yet (e.g. fresh deploy) — silent
    // skip so the page still works off the historical xlsx alone.
    for src in &telraam {
        if let LoaderType::TelraamExcel { segment_id, .. } = &src.loader_type {
            let src_id    = src.id.clone();
            let url       = format!("data/telraam/{}/api.json", segment_id);
            let src_name  = src.name.clone();
            let set_records   = set_records.clone();
            let set_load_msgs = set_load_msgs.clone();
            let set_telraam_api_fetched = set_telraam_api_fetched.clone();
            spawn_local(async move {
                match loader::fetch_telraam_api(&src_id, &url).await {
                    Ok((new_recs, last_mod)) => {
                        if let Some(t) = last_mod {
                            set_telraam_api_fetched.update(|m| { m.insert(src_id.clone(), t); });
                        }
                        if !new_recs.is_empty() {
                            update_date_range(&new_recs, view_mode, date_from, date_to, set_date_from, set_date_to);
                            set_records.update(|r| dedup_extend(r, new_recs));
                        }
                    }
                    Err(e) => {
                        let msg = format!("⚠ {} (API): {}", src_name, e);
                        add_msg(&set_load_msgs, &msg);
                    }
                }
            });
        }
    }

    // ── Fetch each CDN-NDG Excel file ──
    for src in &cdn_ndg {
        if let LoaderType::CdnNdgExcel { file_urls } = &src.loader_type {
            for url in file_urls {
                let src_id    = src.id.clone();
                let url       = url.clone();
                let src_name  = src.name.clone();
                let set_records   = set_records.clone();
                let set_load_msgs = set_load_msgs.clone();
                spawn_local(async move {
                    let msg = format!("⏳ Loading {}…", src_name);
                    add_msg(&set_load_msgs, &msg);
                    match loader::fetch_cdn_ndg_excel(&src_id, &url).await {
                        Ok(new_recs) => {
                            update_date_range(&new_recs, view_mode, date_from, date_to, set_date_from, set_date_to);
                            set_records.update(|r| r.extend(new_recs));
                            remove_msg(&set_load_msgs, &msg);
                        }
                        Err(e) => {
                            replace_msg(&set_load_msgs, &msg,
                                &format!("⚠ {}: {}", src_name, e));
                        }
                    }
                });
            }
        }
    }

    // ── Callbacks ──
    let toggle_source = move |id: String| {
        set_selected_srcs.update(|v| {
            if let Some(i) = v.iter().position(|s| s == &id) { v.remove(i); }
            else { v.push(id); }
        });
    };
    let on_source_toggle = Callback::new(toggle_source.clone());
    let on_map_toggle    = Callback::new(toggle_source);

    let on_modality_toggle = Callback::new(move |m: Modality| {
        set_selected_mods.update(|v| {
            if let Some(i) = v.iter().position(|&x| x == m) {
                if v.len() > 1 { v.remove(i); }
            } else { v.push(m); }
        });
    });

    // Date preset buttons: derived reactively from the loaded records.
    // Includes "All dates", relative ranges (Last Week … Last 6 Months),
    // calendar years, and seasonal Summer/Winter ranges that overlap the data.
    let (date_presets, set_date_presets) =
        signal::<Vec<(String, NaiveDate, NaiveDate, i64, Option<Resolution>)>>(vec![]);
    Effect::new(move |_| {
        let recs = records.get();
        let first = recs.iter().map(|r| r.timestamp.date_naive()).min();
        let last  = recs.iter().map(|r| r.timestamp.date_naive()).max();
        let presets = match (first, last) {
            (Some(f), Some(l)) => compute_date_presets(f, l, lang.get()),
            _ => vec![],
        };
        set_date_presets.set(presets);
    });

    let on_date_preset = Callback::new(
        move |(from, to, force_res): (NaiveDate, NaiveDate, Option<Resolution>)| {
            set_view_mode.set(ViewMode::Linear);
            set_date_from.set(from);
            set_date_to.set(to);
            if let Some(r) = force_res { set_resolution.set(r); }
        },
    );

    // Year-on-Year: anchor a 12-month axis at the earliest record matching
    // the current selection, fold all later years onto that axis, and color
    // each year-bucket distinctly. No-op if the selection is empty or has
    // no records yet.
    let on_year_on_year = Callback::new(move |_: ()| {
        let recs = records.get_untracked();
        let mods = selected_mods.get_untracked();
        let srcs = selected_srcs.get_untracked();
        let earliest = recs.iter()
            .filter(|r| mods.contains(&r.modality) && srcs.contains(&r.source_id))
            .map(|r| r.timestamp)
            .min();
        let Some(start) = earliest else { return };
        let start_d = start.date_naive();
        let end_d = start_d.checked_add_months(Months::new(12)).unwrap_or(start_d);
        set_date_from.set(start_d);
        set_date_to.set(end_d);
        set_view_mode.set(ViewMode::YearOnYear);
    });

    // Winter-on-Winter: filter to the Nov 16 – Mar 31 window, anchor on the
    // earliest winter season represented in the selection, and fold every
    // later winter onto that 4.5-month axis. No-op if the selection has no
    // records inside any winter window.
    let on_winter_on_winter = Callback::new(move |_: ()| {
        let recs = records.get_untracked();
        let mods = selected_mods.get_untracked();
        let srcs = selected_srcs.get_untracked();
        let earliest_winter_year = recs.iter()
            .filter(|r| mods.contains(&r.modality) && srcs.contains(&r.source_id))
            .filter_map(|r| winter_season_year(r.timestamp))
            .min();
        let Some(y) = earliest_winter_year else { return };
        let (sm, sd) = WINTER_START_MD;
        let (em, ed) = WINTER_END_MD;
        let from_d = NaiveDate::from_ymd_opt(y,     sm, sd)
            .expect("Nov 16 is always a valid date");
        let to_d   = NaiveDate::from_ymd_opt(y + 1, em, ed)
            .expect("Mar 31 is always a valid date");
        set_date_from.set(from_d);
        set_date_to.set(to_d);
        set_view_mode.set(ViewMode::WinterOnWinter);
    });

    // ── Derived chart series ──
    let chart_series = move || -> Vec<Series> {
        let recs     = records.get();
        let mods     = selected_mods.get();
        let res      = resolution.get();
        let srcs     = selected_srcs.get();
        let all_srcs = app_sources.get();
        let from_d = date_from.get();
        let to_d   = date_to.get();
        let mode   = view_mode.get();
        let l      = lang.get();

        let from_dt = from_d.and_hms_opt(0, 0, 0)
            .map(|ndt| Utc.from_utc_datetime(&ndt));
        let to_dt = to_d.and_hms_opt(23, 59, 59)
            .map(|ndt| Utc.from_utc_datetime(&ndt));

        match mode {
            ViewMode::Linear => {
                let mut out = vec![];
                for modality in &mods {
                    for src_id in &srcs {
                        if let Some((src_idx, meta)) = all_srcs.iter().enumerate()
                            .find(|(_, s)| &s.id == src_id)
                        {
                            if !meta.modalities.contains(modality) { continue; }
                            let mut pts = loader::aggregate(&recs, *modality, res, Some(src_id));
                            if let Some(f) = from_dt { pts.retain(|(dt, _)| *dt >= f); }
                            if let Some(t) = to_dt   { pts.retain(|(dt, _)| *dt <= t); }
                            if !pts.is_empty() {
                                out.push(Series {
                                    label:  format!("{} – {}", meta.name, modality.label(l)),
                                    color:  series_color(*modality, src_idx),
                                    dash:   modality.stroke_dasharray().unwrap_or("").to_string(),
                                    points: pts,
                                });
                            }
                        }
                    }
                }
                out
            }
            ViewMode::YearOnYear => {
                let Some(start) = from_dt else { return vec![]; };
                let mut out = vec![];
                for modality in &mods {
                    for src_id in &srcs {
                        let Some(meta) = all_srcs.iter().find(|s| &s.id == src_id) else { continue };
                        if !meta.modalities.contains(modality) { continue; }
                        let pts = loader::aggregate(&recs, *modality, res, Some(src_id));
                        if pts.is_empty() { continue; }

                        // Bucket each point by integer years past `start`,
                        // then shift each bucket back onto the [start, +12mo) axis.
                        let mut by_year: BTreeMap<i32, Vec<(DateTime<Utc>, f64)>> = BTreeMap::new();
                        for (t, v) in pts {
                            let yo = year_offset(t, start);
                            if yo < 0 { continue; }
                            by_year.entry(yo).or_default().push((shift_back_years(t, yo), v));
                        }
                        for (yo, year_pts) in by_year {
                            let y0 = start.year() + yo;
                            let year_label = if start.month() == 1 && start.day() == 1 {
                                y0.to_string()
                            } else {
                                format!("{}–{}", y0, y0 + 1)
                            };
                            out.push(Series {
                                label:  format!("{} – {} ({})", meta.name, modality.label(l), year_label),
                                color:  yoy_color(yo),
                                dash:   modality.stroke_dasharray().unwrap_or("").to_string(),
                                points: year_pts,
                            });
                        }
                    }
                }
                out
            }
            ViewMode::WinterOnWinter => {
                // The anchor winter season is determined by the start of the
                // current date window: a `from_dt` of YYYY-11-16 anchors on
                // winter YYYY/YYYY+1.  Each point is bucketed by which winter
                // season it falls into and shifted back so all winters share
                // the same Nov 16 – Mar 31 axis.
                let Some(start) = from_dt else { return vec![]; };
                let anchor_year = start.year();
                let mut out = vec![];
                for modality in &mods {
                    for src_id in &srcs {
                        let Some(meta) = all_srcs.iter().find(|s| &s.id == src_id) else { continue };
                        if !meta.modalities.contains(modality) { continue; }
                        let pts = loader::aggregate(&recs, *modality, res, Some(src_id));
                        if pts.is_empty() { continue; }

                        let mut by_winter: BTreeMap<i32, Vec<(DateTime<Utc>, f64)>> = BTreeMap::new();
                        for (t, v) in pts {
                            let Some(wy) = winter_season_year(t) else { continue };
                            let offset = wy - anchor_year;
                            if offset < 0 { continue; }
                            by_winter.entry(offset).or_default().push((shift_back_years(t, offset), v));
                        }
                        for (offset, winter_pts) in by_winter {
                            let y0 = anchor_year + offset;
                            let label = format!("{}/{}", y0, y0 + 1);
                            out.push(Series {
                                label:  format!("{} – {} ({})", meta.name, modality.label(l), label),
                                color:  yoy_color(offset),
                                dash:   modality.stroke_dasharray().unwrap_or("").to_string(),
                                points: winter_pts,
                            });
                        }
                    }
                }
                out
            }
        }
    };

    let (chart_sig, set_chart_sig) = signal::<Vec<Series>>(vec![]);
    Effect::new(move |_| set_chart_sig.set(chart_series()));

    // X-axis override: a fixed 12-month range when in YearOnYear mode, or
    // the Nov 16 – Mar 31 winter window when in WinterOnWinter mode.  In both
    // cases the visible axis is anchored on `date_from` (set by the callback
    // that switched into the mode) so late-arriving data can't shift it.
    let (x_range_sig, set_x_range_sig) =
        signal::<Option<(DateTime<Utc>, DateTime<Utc>)>>(None);
    Effect::new(move |_| {
        let xr = match view_mode.get() {
            ViewMode::Linear => None,
            ViewMode::YearOnYear => {
                let d = date_from.get();
                d.checked_add_months(Months::new(12)).and_then(|end_d| {
                    let start = Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?);
                    let end   = Utc.from_utc_datetime(&end_d.and_hms_opt(23, 59, 59)?);
                    Some((start, end))
                })
            }
            ViewMode::WinterOnWinter => {
                let d = date_from.get();
                let (em, ed) = WINTER_END_MD;
                NaiveDate::from_ymd_opt(d.year() + 1, em, ed).and_then(|end_d| {
                    let start = Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?);
                    let end   = Utc.from_utc_datetime(&end_d.and_hms_opt(23, 59, 59)?);
                    Some((start, end))
                })
            }
        };
        set_x_range_sig.set(xr);
    });

    // Status bar: show all in-flight or error messages
    let status_text = move || {
        let msgs = load_msgs.get();
        if msgs.is_empty() { String::new() } else { msgs.join("  ") }
    };

    // Format the cron-written status line for display:
    //   1. Strip any legacy "VdM data: " prefix (still present until cron
    //      next runs after the script change that emits a bare timestamp).
    //   2. Parse the timestamp as RFC 3339 UTC and convert to the browser's
    //      local timezone so users see their own clock instead of UTC.
    //   3. Fall back to showing the raw string if anything fails (defensive
    //      against an unexpected server-side format change).
    let localized_status = move || {
        let raw = data_status.get().unwrap_or_default();
        if raw.is_empty() { return String::new(); }
        let body = raw.trim().strip_prefix("VdM data: ").unwrap_or(raw.trim());
        let display = match chrono::DateTime::parse_from_rfc3339(body) {
            Ok(dt) => dt.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string(),
            Err(_) => body.to_string(),
        };
        format!("{}: {}", lang.get().t().vdm_data_prefix, display)
    };

    // VdM hover tooltip: per-intersection last bike record, parallel to the
    // Telraam tooltip. The visible header value already shows the cron's
    // last successful download time; this breakdown lets the user see how
    // far behind each individual intersection is — which can lag the cron
    // freshness by hours or days when the city batches uploads.
    let vdm_tooltip = move || -> String {
        let recs = records.get();
        let srcs = app_sources.get();
        let t = lang.get().t();
        let na = t.value_unavailable;
        let fmt_local = |dt: DateTime<Utc>| dt.with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M").to_string();

        // Group VdM sources by their `group` key (= intersection total_id).
        // Each group's display label comes from the Total source when one
        // exists; falls back to the directional source's name with the
        // direction suffix stripped, for single-direction intersections.
        let mut group_ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut group_label: BTreeMap<String, String> = BTreeMap::new();
        for s in srcs.iter().filter(|s| s.id.starts_with("mtl-")) {
            let Some(g) = s.group.clone() else { continue };
            group_ids.entry(g.clone()).or_default().push(s.id.clone());
            let is_total = s.id == g;
            if is_total || !group_label.contains_key(&g) {
                let n = s.name.strip_prefix("VdM: ").unwrap_or(&s.name);
                let n = n.strip_suffix(" (Total)").unwrap_or(n);
                let n = n.split(" (").next().unwrap_or(n);
                if is_total {
                    group_label.insert(g.clone(), n.to_string());
                } else {
                    group_label.entry(g.clone()).or_insert_with(|| n.to_string());
                }
            }
        }

        let raw = data_status.get().unwrap_or_default();
        let body = raw.trim().strip_prefix("VdM data: ").unwrap_or(raw.trim());
        let download_time = match chrono::DateTime::parse_from_rfc3339(body) {
            Ok(dt) => dt.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string(),
            Err(_) if body.is_empty() => na.to_string(),
            Err(_) => body.to_string(),
        };

        let mut lines = vec![
            format!("{}: {}", t.last_successful_download, download_time),
        ];
        if !group_ids.is_empty() {
            lines.push(format!("{}:", t.last_bike_record));
            for (g, ids) in &group_ids {
                let label = group_label.get(g).cloned().unwrap_or_else(|| g.clone());
                let max_ts = recs.iter()
                    .filter(|r| ids.iter().any(|id| id == &r.source_id)
                            && r.modality == Modality::Bikes
                            && r.count > 0.0)
                    .map(|r| r.timestamp).max();
                let val = max_ts.map(fmt_local).unwrap_or_else(|| na.to_string());
                lines.push(format!("  {}: {}", label, val));
            }
        }
        lines.join("\n")
    };

    // Per-segment Telraam freshness. Each entry surfaces three timestamps so
    // users can disambiguate cron health from sensor health:
    //   - last_fetch:  api.json mtime (cron success — independent of sensor)
    //   - last_record: newest hour with any data (sensor heartbeat)
    //   - last_bike:   newest hour where bikes > 0 (last observed traffic)
    //
    // The visible header value is `last_record` — it ticks hourly while the
    // sensor is alive even on quiet (zero-bike) hours. The tooltip exposes
    // all three. The stale flag fires when the sensor has been silent for
    // several hours despite a recent successful fetch (i.e. cron is fine
    // but the sensor itself stopped reporting).
    let telraam_freshness = move || -> Vec<TelraamSegStatus> {
        let recs = records.get();
        let srcs = app_sources.get();
        let fetched = telraam_api_fetched.get();
        let segs: Vec<(String, String)> = srcs.iter()
            .filter(|s| matches!(s.loader_type, LoaderType::TelraamExcel { .. }))
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();

        let now = Utc::now();
        let mut out = Vec::with_capacity(segs.len());
        for (id, name) in &segs {
            let prefix = format!("{}-", id);
            let in_seg = |r: &CountRecord|
                r.source_id == *id || r.source_id.starts_with(&prefix);
            let last_record = recs.iter()
                .filter(|r| in_seg(r))
                .map(|r| r.timestamp).max();
            let last_bike = recs.iter()
                .filter(|r| in_seg(r) && r.modality == Modality::Bikes && r.count > 0.0)
                .map(|r| r.timestamp).max();
            let last_fetch = fetched.get(id).copied();
            let is_stale = match last_record {
                Some(t) => (now - t) > Duration::hours(STALE_HOURS),
                None    => false,
            };
            let full_name = name.strip_suffix(" — Total")
                .unwrap_or(name).to_string();
            let ann = telraam_annotation(id);
            let link_url = ann
                .map(|a| telraam_segment_url(a.api_id))
                .unwrap_or_else(|| "https://telraam.net/".to_string());
            let cross = cross_street(name);
            let short_label = match ann {
                Some(a) => format!("{}@{}", a.street_abbrev, cross),
                None    => cross,
            };
            out.push(TelraamSegStatus {
                short_label,
                full_name,
                last_record, last_bike, last_fetch, is_stale,
                link_url,
            });
        }
        out
    };

    view! {
        <div id="app" class:sidebar-open=move || sidebar_open.get()>
            <header>
                <button class="mobile-toggle"
                        on:click=move |_| set_sidebar_open.update(|v| *v = !*v)>
                    {move || {
                        let t = lang.get().t();
                        if sidebar_open.get() { t.mobile_close } else { t.mobile_filters }
                    }}
                </button>
                <h1>"BikeStat"</h1>
                <span class="subtitle">{move || lang.get().t().subtitle}</span>
                <span class="load-status">{status_text}</span>
                <span class="data-status">
                    <a class="vdm-link" title=vdm_tooltip
                       href=VDM_DATASET_URL
                       target="_blank" rel="noopener noreferrer">
                        {localized_status}
                    </a>
                </span>
                <span class="data-status">
                    {move || {
                        let segs = telraam_freshness();
                        if segs.is_empty() { return None; }
                        let t = lang.get().t();
                        let prefix = t.telraam_data_prefix;
                        let na = t.value_unavailable;
                        let label_fetch  = t.last_api_fetch;
                        let label_record = t.last_record;
                        let label_bike   = t.last_bike;
                        let fmt_local = |dt: DateTime<Utc>| dt.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M").to_string();
                        let last = segs.len().saturating_sub(1);
                        let entries: Vec<_> = segs.into_iter().enumerate().map(|(i, s)| {
                            let value = s.last_record.map(fmt_local)
                                .unwrap_or_else(|| na.to_string());
                            let display = if s.is_stale {
                                format!("{} {} ⚠", s.short_label, value)
                            } else {
                                format!("{} {}", s.short_label, value)
                            };
                            let tooltip = format!(
                                "{}\n  {}: {}\n  {}: {}\n  {}: {}",
                                s.full_name,
                                label_fetch,  s.last_fetch.map(fmt_local)
                                    .unwrap_or_else(|| na.to_string()),
                                label_record, s.last_record.map(fmt_local)
                                    .unwrap_or_else(|| na.to_string()),
                                label_bike,   s.last_bike.map(fmt_local)
                                    .unwrap_or_else(|| na.to_string()),
                            );
                            let sep = if i < last { Some(" · ") } else { None };
                            view! {
                                <a class="telraam-seg" class:stale=s.is_stale
                                   title=tooltip
                                   href=s.link_url
                                   target="_blank" rel="noopener noreferrer">
                                    {display}
                                </a>
                                {sep}
                            }
                        }).collect();
                        Some(view! {
                            <>
                                {format!("{}: ", prefix)}
                                {entries}
                            </>
                        })
                    }}
                </span>
                <button class="lang-toggle"
                        title="Language / Langue"
                        on:click=move |_| set_lang.update(|l| *l = l.other())>
                    {move || lang.get().other().short_label()}
                </button>
            </header>

            <Sidebar
                sources=app_sources
                selected_sources=selected_srcs
                on_source_toggle=on_source_toggle

                resolution=resolution
                on_resolution=Callback::new(move |r| set_resolution.set(r))

                selected_modalities=selected_mods
                on_modality_toggle=on_modality_toggle

                date_from=date_from
                date_to=date_to
                on_date_from=Callback::new(move |d| set_date_from.set(d))
                on_date_to=Callback::new(move |d| set_date_to.set(d))

                date_presets=date_presets
                on_date_preset=on_date_preset

                view_mode=view_mode
                on_year_on_year=on_year_on_year
                on_winter_on_winter=on_winter_on_winter
            />

            <main>
                <Chart series=chart_sig x_range=x_range_sig resolution=resolution />
                <SourceMap
                    sources=app_sources
                    selected=selected_srcs
                    on_toggle=on_map_toggle
                />
            </main>
        </div>
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Hours of silence (no records produced for a Telraam segment) before the
/// header bar flags it stale. A live sensor reports every hour even on
/// quiet ones, so anything beyond a few hours indicates a real outage.
const STALE_HOURS: i64 = 4;

/// Per-segment Telraam freshness data, surfaced in the header bar.
struct TelraamSegStatus {
    /// Short label for visible display (e.g. "King Edward").
    short_label: String,
    /// Full source name for the tooltip header.
    full_name:   String,
    /// Newest hour with any record — sensor heartbeat.
    last_record: Option<DateTime<Utc>>,
    /// Newest hour with bikes > 0 — last observed bike traffic.
    last_bike:   Option<DateTime<Utc>>,
    /// `Last-Modified` of the cron-written api.json — last successful fetch.
    last_fetch:  Option<DateTime<Utc>>,
    /// True when `last_record` is older than `STALE_HOURS` hours.
    is_stale:    bool,
    /// Public telraam.net page for this segment, if the annotation includes
    /// an `api_id`. Empty string falls back to the Telraam home page so the
    /// chip is still navigable (and the screen reader still announces it as
    /// a link) for any future segment lacking an api_id.
    link_url:    String,
}

/// Public dataset page for the Ville de Montréal cyclistes feed. Linked
/// from the VdM freshness chip as attribution and a path back to the raw
/// CSV / metadata.
const VDM_DATASET_URL: &str = "https://donnees.montreal.ca/dataset/cyclistes";

/// Build a `https://telraam.net/en/location/<api_id>/<from>/<to>` URL for a
/// 7-day window ending yesterday — matches the format Telraam's own date
/// picker emits and gives the user a useful default view of the segment.
fn telraam_segment_url(api_id: &str) -> String {
    let today = chrono::Local::now().date_naive();
    let end   = today - chrono::Duration::days(1);
    let start = today - chrono::Duration::days(7);
    format!("https://telraam.net/en/location/{}/{}/{}",
        api_id,
        start.format("%Y-%m-%d"),
        end.format("%Y-%m-%d"))
}

/// Cross-street of a Telraam segment, parsed from the part of `full_name`
/// after " @ " with the " — Total" suffix dropped. Combined with the
/// annotation's `street_abbrev` to form a header chip label like
/// `"TB@King Edward"`. Falls back to the full name when the convention
/// isn't followed (e.g. for sources without a cross-street).
fn cross_street(full_name: &str) -> String {
    let cross = full_name.rsplit_once(" @ ")
        .map(|(_, c)| c)
        .unwrap_or(full_name);
    cross.strip_suffix(" — Total").unwrap_or(cross).to_string()
}

/// Expand the visible date window to include all timestamps in `recs`.
/// Never shrinks an existing bound — only moves `from` earlier or `to` later.
/// Skipped while in YearOnYear or WinterOnWinter mode so late-arriving data
/// doesn't shift the fixed comparison axis out from under the user.
fn update_date_range(
    recs: &[CountRecord],
    view_mode: ReadSignal<ViewMode>,
    date_from: ReadSignal<NaiveDate>,
    date_to:   ReadSignal<NaiveDate>,
    set_from:  WriteSignal<NaiveDate>,
    set_to:    WriteSignal<NaiveDate>,
) {
    let mode = view_mode.get_untracked();
    if matches!(mode, ViewMode::YearOnYear | ViewMode::WinterOnWinter) { return; }

    let (Some(new_first), Some(new_last)) = (
        recs.iter().map(|r| r.timestamp.date_naive()).min(),
        recs.iter().map(|r| r.timestamp.date_naive()).max(),
    ) else { return };

    set_from.set(date_from.get_untracked().min(new_first));
    set_to.set(date_to.get_untracked().max(new_last));
}

/// Append `new_recs` to `records`, skipping any whose `(source_id,
/// timestamp, modality)` key already exists.  Telraam's API window can
/// overlap the historical xlsx range; without dedup the chart would
/// double-count those hours.  Whichever loader writes first wins —
/// acceptable since the legacy and S2 sensors should report comparable
/// values for the same physical location.
fn dedup_extend(records: &mut Vec<CountRecord>, new_recs: Vec<CountRecord>) {
    use std::collections::HashSet;
    let mut seen: HashSet<(String, i64, Modality)> = records.iter()
        .map(|r| (r.source_id.clone(), r.timestamp.timestamp(), r.modality))
        .collect();
    for rec in new_recs {
        let key = (rec.source_id.clone(), rec.timestamp.timestamp(), rec.modality);
        if seen.insert(key) {
            records.push(rec);
        }
    }
}

fn add_msg(signal: &WriteSignal<Vec<String>>, msg: &str) {
    let s = msg.to_string();
    signal.update(|v| v.push(s));
}

fn remove_msg(signal: &WriteSignal<Vec<String>>, msg: &str) {
    signal.update(|v| v.retain(|m| m != msg));
}

fn replace_msg(signal: &WriteSignal<Vec<String>>, old: &str, new: &str) {
    let (o, n) = (old.to_string(), new.to_string());
    signal.update(|v| {
        if let Some(i) = v.iter().position(|m| m == &o) { v[i] = n; }
        else { v.push(n); }
    });
}

/// Build the list of `(label, from, to, days)` preset ranges for the data
/// window [from_str, to_str]. The 4th tuple field is the duration in days,
/// which the sidebar uses to disable presets that are too short to produce
/// a non-empty chart at the current resolution (e.g. "Last Week" + Month).
/// Each preset is only emitted when it overlaps the available data, so
/// users only see buttons that will actually do something.
///
/// Order:
///   1. "All dates" — the full data extent.
///   2. Relative — Last 48H / Week / Month / 3 Months / 6 Months / Year,
///      anchored at the latest record. Skipped if the resulting start would
///      precede the data.
///   3. Calendar years — one per year touched by the data window
///      (nominal Jan 1 → Dec 31; chart filtering handles partial coverage).
///   4. Seasonal — Summer (Apr 1 → Nov 15) and Winter (Nov 16 → Mar 31 of
///      the following year) entries that overlap the data, sorted by start.
fn compute_date_presets(
    data_from: NaiveDate,
    data_to:   NaiveDate,
    lang:      Lang,
) -> Vec<(String, NaiveDate, NaiveDate, i64, Option<Resolution>)> {
    let t = lang.t();

    let entry = |label: &str, f: NaiveDate, tdate: NaiveDate, force_res: Option<Resolution>| {
        (label.to_string(), f, tdate, (tdate - f).num_days(), force_res)
    };

    let mut out = Vec::new();

    // ── All dates ──
    out.push(entry(t.all_dates, data_from, data_to, None));

    // ── Relative presets, anchored at the latest record ──
    // "Last 48H" subtracts a single day so the inclusive [from 00:00, to 23:59]
    // window spans 48 hours of data, and forces Hour resolution since a daily
    // bar chart of two days is rarely what the user wants. Disabled at Week /
    // Month resolutions by the sidebar's days < min_days check.
    let relatives: [(&str, Option<NaiveDate>, Option<Resolution>); 6] = [
        (t.last_48h,      Some(data_to - Duration::days(1)),                         Some(Resolution::Hour)),
        (t.last_week,     Some(data_to - Duration::days(7)),                         None),
        (t.last_month,    data_to.checked_sub_months(Months::new(1)),                None),
        (t.last_3_months, data_to.checked_sub_months(Months::new(3)),                None),
        (t.last_6_months, data_to.checked_sub_months(Months::new(6)),                None),
        (t.last_year,     data_to.checked_sub_months(Months::new(12))
                                  .map(|d| d + Duration::days(1)),                   None),
    ];
    for (label, from_opt, force_res) in relatives {
        if let Some(from_dt) = from_opt {
            if from_dt >= data_from {
                out.push(entry(label, from_dt, data_to, force_res));
            }
        }
    }

    // ── Calendar year presets ──
    for y in data_from.year()..=data_to.year() {
        if let (Some(y_start), Some(y_end)) = (
            NaiveDate::from_ymd_opt(y, 1, 1),
            NaiveDate::from_ymd_opt(y, 12, 31),
        ) {
            if y_end >= data_from && y_start <= data_to {
                out.push(entry(&y.to_string(), y_start, y_end, None));
            }
        }
    }

    // ── Seasonal presets ──
    let mut seasons = Vec::new();
    for y in (data_from.year() - 1)..=(data_to.year() + 1) {
        if let (Some(sf), Some(st)) = (
            NaiveDate::from_ymd_opt(y, 4,  1),
            NaiveDate::from_ymd_opt(y, 11, 15),
        ) {
            if st >= data_from && sf <= data_to {
                seasons.push(entry(&format!("{} {}", t.summer, y), sf, st, None));
            }
        }
        if let (Some(wf), Some(wt)) = (
            NaiveDate::from_ymd_opt(y,     11, 16),
            NaiveDate::from_ymd_opt(y + 1, 3,  31),
        ) {
            if wt >= data_from && wf <= data_to {
                seasons.push(entry(&format!("{} {}/{}", t.winter, y, y + 1), wf, wt, None));
            }
        }
    }
    seasons.sort_by(|a, b| a.1.cmp(&b.1));
    out.extend(seasons);

    out
}

/// Composite series color: source index sets the hue, modality sets lightness.
///
/// Locations get distinct hues so overlay comparison across sources is the
/// primary visual cue.  Modality is encoded primarily by line dash pattern;
/// the lightness offset here is a secondary tiebreaker when several
/// modalities for the same location are plotted together.
///
/// Use case 1 (same modality, different locations): hues are distinct.
/// Use case 2 (same location, different modalities): same hue, lightness varies.
fn series_color(modality: Modality, source_idx: usize) -> String {
    // Eight distinct hues spread around the wheel, hand-picked for legibility
    // against the dark theme.  Beyond eight sources we cycle.
    const HUES: &[f64] = &[350.0, 30.0, 70.0, 120.0, 175.0, 215.0, 260.0, 310.0];
    let hue = HUES[source_idx % HUES.len()];
    let lightness = match modality {
        Modality::Trucks      => 0.42,
        Modality::Pedestrians => 0.52,
        Modality::Bikes       => 0.62,
        Modality::Cars        => 0.72,
    };
    hsl_to_hex(hue, 0.72, lightness)
}

/// Number of full 12-month periods between `start` and `t`, using calendar
/// month/day comparison (so leap years don't shift the boundary).  Negative
/// when `t` is before `start`.
fn year_offset(t: DateTime<Utc>, start: DateTime<Utc>) -> i32 {
    let mut offset = t.year() - start.year();
    if (t.month(), t.day()) < (start.month(), start.day()) {
        offset -= 1;
    }
    offset
}

/// Subtract `years` calendar years from `t`. Uses chrono's `Months` so a
/// Feb 29 in a leap year maps to Feb 28 in non-leap years rather than failing.
fn shift_back_years(t: DateTime<Utc>, years: i32) -> DateTime<Utc> {
    if years <= 0 { return t; }
    let date = t.date_naive()
        .checked_sub_months(Months::new(12 * years as u32))
        .unwrap_or_else(|| t.date_naive());
    Utc.from_utc_datetime(&date.and_time(t.time()))
}

// ── Winter-on-Winter helpers ─────────────────────────────────────────────────
//
// A "winter" is the period from Nov 16 of year Y to Mar 31 of year Y+1
// (~4.5 months).  Each winter is identified by its starting calendar year Y
// and labelled "Winter Y/Y+1".  These helpers let the chart filter records
// to the winter window and align successive winters on a single axis.

/// First day of the winter window (inclusive), expressed as (month, day).
const WINTER_START_MD: (u32, u32) = (11, 16);
/// Last day of the winter window (inclusive), expressed as (month, day).
const WINTER_END_MD:   (u32, u32) = (3, 31);

/// Calendar year `Y` such that `t` belongs to "Winter Y/Y+1", or `None` if
/// `t` is outside the Nov 16 – Mar 31 window.
fn winter_season_year(t: DateTime<Utc>) -> Option<i32> {
    let d = t.date_naive();
    let (m, day) = (d.month(), d.day());
    let (sm, sd) = WINTER_START_MD;
    let (em, _ed) = WINTER_END_MD;
    if m > sm || (m == sm && day >= sd) {
        // Nov 16 – Dec 31: winter starting this year
        Some(d.year())
    } else if m < em || m == em {
        // Jan 1 – Mar 31: winter that started last November
        Some(d.year() - 1)
    } else {
        None
    }
}

/// Distinct-hue palette for Year-on-Year buckets, tuned for the dark theme.
fn yoy_color(year_offset: i32) -> String {
    const HUES: &[f64] = &[210.0, 30.0, 130.0, 290.0, 0.0, 60.0];
    let h = HUES[year_offset.unsigned_abs() as usize % HUES.len()];
    hsl_to_hex(h, 0.70, 0.62)
}

fn hsl_to_hex(h: f64, s: f64, l: f64) -> String {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h6 = h / 60.0;
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = if      h6 < 1.0 { (c, x, 0.0) }
                    else if h6 < 2.0 { (x, c, 0.0) }
                    else if h6 < 3.0 { (0.0, c, x) }
                    else if h6 < 4.0 { (0.0, x, c) }
                    else if h6 < 5.0 { (x, 0.0, c) }
                    else             { (c, 0.0, x) };
    let u = |v: f64| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", u(r), u(g), u(b))
}
