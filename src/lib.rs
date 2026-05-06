mod data;
mod components;

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, Months, NaiveDate, TimeZone, Utc};
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

use data::{loader, sources};
use data::types::{CountRecord, DataSource, LoaderType, Modality, Resolution, ViewMode};
use components::chart::{Chart, Series};
use components::map::SourceMap;
use components::sidebar::Sidebar;

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

    let (selected_srcs, set_selected_srcs) = signal::<Vec<String>>(vec![]);
    let (selected_mods, set_selected_mods) = signal::<Vec<Modality>>(vec![Modality::Bikes]);
    let (resolution,    set_resolution)    = signal(Resolution::Day);
    let (view_mode,     set_view_mode)     = signal(ViewMode::Linear);

    let now = Utc::now();
    let (date_from, set_date_from) = signal(format!("{}-01-01", now.year()));
    let (date_to,   set_date_to)   = signal(format!("{}-12-31", now.year()));

    // ── Seed catalogue with pre-configured sources immediately ──
    // Their records load asynchronously below; the entries appear in the
    // sidebar right away so the user can see what is expected.
    let telraam = sources::telraam_sources();
    let cdn_ndg = sources::cdn_ndg_sources();
    set_sources.update(|s| {
        s.extend(telraam.clone());
        s.extend(cdn_ndg.clone());
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
        signal::<Vec<(String, String, String)>>(vec![]);
    Effect::new(move |_| {
        let recs = records.get();
        let first = recs.iter().map(|r| r.timestamp).min();
        let last  = recs.iter().map(|r| r.timestamp).max();
        let presets = match (first, last) {
            (Some(f), Some(l)) => compute_date_presets(
                &f.format("%Y-%m-%d").to_string(),
                &l.format("%Y-%m-%d").to_string(),
            ),
            _ => vec![],
        };
        set_date_presets.set(presets);
    });

    let on_date_preset = Callback::new(move |(from, to): (String, String)| {
        set_view_mode.set(ViewMode::Linear);
        set_date_from.set(from);
        set_date_to.set(to);
    });

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
        set_date_from.set(start_d.format("%Y-%m-%d").to_string());
        set_date_to.set(end_d.format("%Y-%m-%d").to_string());
        set_view_mode.set(ViewMode::YearOnYear);
    });

    // ── Derived chart series ──
    let chart_series = move || -> Vec<Series> {
        let recs     = records.get();
        let mods     = selected_mods.get();
        let res      = resolution.get();
        let srcs     = selected_srcs.get();
        let all_srcs = app_sources.get();
        let from_str = date_from.get();
        let to_str   = date_to.get();
        let mode     = view_mode.get();

        let from_dt = NaiveDate::parse_from_str(&from_str, "%Y-%m-%d")
            .ok().and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|ndt| Utc.from_utc_datetime(&ndt));
        let to_dt = NaiveDate::parse_from_str(&to_str, "%Y-%m-%d")
            .ok().and_then(|d| d.and_hms_opt(23, 59, 59))
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
                                    label:  format!("{} – {}", meta.name, modality.label()),
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
                                label:  format!("{} – {} ({})", meta.name, modality.label(), year_label),
                                color:  yoy_color(yo),
                                dash:   modality.stroke_dasharray().unwrap_or("").to_string(),
                                points: year_pts,
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

    // X-axis override: a fixed 12-month range when in YearOnYear mode.
    let (x_range_sig, set_x_range_sig) =
        signal::<Option<(DateTime<Utc>, DateTime<Utc>)>>(None);
    Effect::new(move |_| {
        let xr = if view_mode.get() == ViewMode::YearOnYear {
            NaiveDate::parse_from_str(&date_from.get(), "%Y-%m-%d").ok()
                .and_then(|d| {
                    let end_d = d.checked_add_months(Months::new(12))?;
                    let start = Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?);
                    let end   = Utc.from_utc_datetime(&end_d.and_hms_opt(23, 59, 59)?);
                    Some((start, end))
                })
        } else { None };
        set_x_range_sig.set(xr);
    });

    // Status bar: show all in-flight or error messages
    let status_text = move || {
        let msgs = load_msgs.get();
        if msgs.is_empty() { String::new() } else { msgs.join("  ") }
    };

    view! {
        <div id="app">
            <header>
                <h1>"BikeStat"</h1>
                <span class="subtitle">"Traffic Count Aggregator"</span>
                <span class="load-status">{status_text}</span>
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
                on_date_from=Callback::new(move |s| set_date_from.set(s))
                on_date_to=Callback::new(move |s| set_date_to.set(s))

                date_presets=date_presets
                on_date_preset=on_date_preset

                view_mode=view_mode
                on_year_on_year=on_year_on_year
            />

            <main>
                <Chart series=chart_sig x_range=x_range_sig />
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

/// Expand the visible date window to include all timestamps in `recs`.
/// Never shrinks an existing bound — only moves `from` earlier or `to` later.
/// Skipped while in YearOnYear mode so late-arriving data doesn't shift the
/// 12-month axis out from under the user.
fn update_date_range(
    recs: &[CountRecord],
    view_mode: ReadSignal<ViewMode>,
    date_from: ReadSignal<String>,
    date_to:   ReadSignal<String>,
    set_from:  WriteSignal<String>,
    set_to:    WriteSignal<String>,
) {
    if view_mode.get_untracked() == ViewMode::YearOnYear { return; }

    let (Some(new_first), Some(new_last)) = (
        recs.iter().map(|r| r.timestamp).min(),
        recs.iter().map(|r| r.timestamp).max(),
    ) else { return };

    let parse = |s: String| NaiveDate::parse_from_str(&s, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|ndt| Utc.from_utc_datetime(&ndt));

    let from = parse(date_from.get_untracked())
        .map_or(new_first, |cur| cur.min(new_first));
    let to   = parse(date_to.get_untracked())
        .map_or(new_last,  |cur| cur.max(new_last));

    set_from.set(from.format("%Y-%m-%d").to_string());
    set_to.set(to.format("%Y-%m-%d").to_string());
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

/// Build the list of `(label, from, to)` preset ranges for the data window
/// [from_str, to_str]. Each preset is only emitted when it overlaps the
/// available data, so users only see buttons that will actually do something.
///
/// Order:
///   1. "All dates" — the full data extent.
///   2. Relative — Last Week / Month / 3 Months / 6 Months, anchored at the
///      latest record. Skipped if the resulting start would precede the data.
///   3. Calendar years — one per year touched by the data window
///      (nominal Jan 1 → Dec 31; chart filtering handles partial coverage).
///   4. Seasonal — Summer (Apr 1 → Nov 15) and Winter (Nov 16 → Mar 31 of
///      the following year) entries that overlap the data, sorted by start.
fn compute_date_presets(from_str: &str, to_str: &str) -> Vec<(String, String, String)> {
    let Ok(data_from) = NaiveDate::parse_from_str(from_str, "%Y-%m-%d") else { return vec![] };
    let Ok(data_to)   = NaiveDate::parse_from_str(to_str,   "%Y-%m-%d") else { return vec![] };

    let mut out = Vec::new();
    let to_iso = data_to.format("%Y-%m-%d").to_string();

    // ── All dates ──
    out.push((
        "All dates".to_string(),
        data_from.format("%Y-%m-%d").to_string(),
        to_iso.clone(),
    ));

    // ── Relative presets, anchored at the latest record ──
    let relatives: [(&str, Option<NaiveDate>); 4] = [
        ("Last Week",     Some(data_to - Duration::days(7))),
        ("Last Month",    data_to.checked_sub_months(Months::new(1))),
        ("Last 3 Months", data_to.checked_sub_months(Months::new(3))),
        ("Last 6 Months", data_to.checked_sub_months(Months::new(6))),
    ];
    for (label, from_opt) in relatives {
        if let Some(from_dt) = from_opt {
            if from_dt >= data_from {
                out.push((
                    label.to_string(),
                    from_dt.format("%Y-%m-%d").to_string(),
                    to_iso.clone(),
                ));
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
                out.push((
                    y.to_string(),
                    y_start.format("%Y-%m-%d").to_string(),
                    y_end.format("%Y-%m-%d").to_string(),
                ));
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
                seasons.push((
                    format!("Summer {}", y),
                    sf.format("%Y-%m-%d").to_string(),
                    st.format("%Y-%m-%d").to_string(),
                ));
            }
        }
        if let (Some(wf), Some(wt)) = (
            NaiveDate::from_ymd_opt(y,     11, 16),
            NaiveDate::from_ymd_opt(y + 1, 3,  31),
        ) {
            if wt >= data_from && wf <= data_to {
                seasons.push((
                    format!("Winter {}/{}", y, y + 1),
                    wf.format("%Y-%m-%d").to_string(),
                    wt.format("%Y-%m-%d").to_string(),
                ));
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
        Modality::Motorcycles => 0.80,
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
