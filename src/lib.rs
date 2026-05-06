mod data;
mod components;

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

use data::{loader, sources};
use data::types::{CountRecord, DataSource, LoaderType, Modality, Resolution};
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

    let now = Utc::now();
    let (date_from, set_date_from) = signal(format!("{}-01-01", now.year()));
    let (date_to,   set_date_to)   = signal(format!("{}-12-31", now.year()));

    // ── Seed catalogue with pre-configured Telraam sources immediately ──
    // Their records load asynchronously below; the entries appear in the
    // sidebar right away so the user can see what is expected.
    let telraam = sources::telraam_sources();
    set_sources.update(|s| s.extend(telraam.clone()));

    // ── Fetch Montreal data ──
    {
        let set_sources   = set_sources.clone();
        let set_records   = set_records.clone();
        let set_load_msgs = set_load_msgs.clone();
        spawn_local(async move {
            add_msg(&set_load_msgs, "⏳ Loading Montréal data…");
            match loader::fetch_montreal_cyclistes().await {
                Ok((new_srcs, new_recs)) => {
                    update_date_range(&new_recs, date_from, date_to, set_date_from, set_date_to);
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
                            update_date_range(&new_recs, date_from, date_to, set_date_from, set_date_to);
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

    // "All dates" resets the view to the full extent of whatever is loaded.
    let on_preset = Callback::new(move |_: &'static str| {
        let recs = records.get_untracked();
        if let (Some(first), Some(last)) = (
            recs.iter().map(|r| r.timestamp).min(),
            recs.iter().map(|r| r.timestamp).max(),
        ) {
            set_date_from.set(first.format("%Y-%m-%d").to_string());
            set_date_to.set(last.format("%Y-%m-%d").to_string());
        }
    });

    // Season preset buttons: derived reactively from the loaded records.
    let (season_presets, set_season_presets) =
        signal::<Vec<(String, String, String)>>(vec![]);
    Effect::new(move |_| {
        let recs = records.get();
        let first = recs.iter().map(|r| r.timestamp).min();
        let last  = recs.iter().map(|r| r.timestamp).max();
        let presets = match (first, last) {
            (Some(f), Some(l)) => compute_season_presets(
                &f.format("%Y-%m-%d").to_string(),
                &l.format("%Y-%m-%d").to_string(),
            ),
            _ => vec![],
        };
        set_season_presets.set(presets);
    });

    let on_season = Callback::new(move |(from, to): (String, String)| {
        set_date_from.set(from);
        set_date_to.set(to);
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

        let from_dt = NaiveDate::parse_from_str(&from_str, "%Y-%m-%d")
            .ok().and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|ndt| Utc.from_utc_datetime(&ndt));
        let to_dt = NaiveDate::parse_from_str(&to_str, "%Y-%m-%d")
            .ok().and_then(|d| d.and_hms_opt(23, 59, 59))
            .map(|ndt| Utc.from_utc_datetime(&ndt));

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
    };

    let (chart_sig, set_chart_sig) = signal::<Vec<Series>>(vec![]);
    Effect::new(move |_| set_chart_sig.set(chart_series()));

    // Status bar: show all in-flight or error messages
    let status_text = move || {
        let msgs = load_msgs.get();
        if msgs.is_empty() { String::new() } else { msgs.join("  ") }
    };

    view! {
        <div id="app">
            <header>
                <h1>"BikeStat"</h1>
                <span class="subtitle">"Traffic Count Explorer"</span>
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

                on_preset=on_preset
                season_presets=season_presets
                on_season=on_season
            />

            <main>
                <Chart series=chart_sig />
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
fn update_date_range(
    recs: &[CountRecord],
    date_from: ReadSignal<String>,
    date_to:   ReadSignal<String>,
    set_from:  WriteSignal<String>,
    set_to:    WriteSignal<String>,
) {
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

/// Composite series color: modality sets the hue, source index sets lightness.
///
/// Use case 1 (same modality, different locations): same hue, lightness varies.
/// Use case 2 (same location, different modalities): hues are distinct.
/// Return one `(label, from, to)` entry per summer and winter season that
/// overlaps with the data window [from_str, to_str].
///
/// Season boundaries follow Montréal's seasonal bike-lane calendar:
///   Summer — Apr 1 → Nov 15  (label "Summer YYYY")
///   Winter — Nov 16 → Mar 31 the following year  (label "Winter YYYY/YYYY+1")
fn compute_season_presets(from_str: &str, to_str: &str) -> Vec<(String, String, String)> {
    let Ok(data_from) = NaiveDate::parse_from_str(from_str, "%Y-%m-%d") else { return vec![] };
    let Ok(data_to)   = NaiveDate::parse_from_str(to_str,   "%Y-%m-%d") else { return vec![] };

    let mut out = Vec::new();
    // Scan years generously so we catch winters that straddle year boundaries.
    for y in (data_from.year() - 1)..=(data_to.year() + 1) {
        // Summer
        if let (Some(sf), Some(st)) = (
            NaiveDate::from_ymd_opt(y, 4,  1),
            NaiveDate::from_ymd_opt(y, 11, 15),
        ) {
            if st >= data_from && sf <= data_to {
                out.push((
                    format!("Summer {}", y),
                    sf.format("%Y-%m-%d").to_string(),
                    st.format("%Y-%m-%d").to_string(),
                ));
            }
        }
        // Winter (Nov 16 of y → Mar 31 of y+1)
        if let (Some(wf), Some(wt)) = (
            NaiveDate::from_ymd_opt(y,     11, 16),
            NaiveDate::from_ymd_opt(y + 1, 3,  31),
        ) {
            if wt >= data_from && wf <= data_to {
                out.push((
                    format!("Winter {}/{}", y, y + 1),
                    wf.format("%Y-%m-%d").to_string(),
                    wt.format("%Y-%m-%d").to_string(),
                ));
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn series_color(modality: Modality, source_idx: usize) -> String {
    let (hue, sat) = match modality {
        Modality::Bikes       => (350.0_f64, 0.80),
        Modality::Pedestrians => ( 35.0,     0.85),
        Modality::Cars        => (210.0,     0.75),
        Modality::Trucks      => (120.0,     0.65),
        Modality::Motorcycles => (280.0,     0.75),
    };
    // Lightness steps chosen for dark-theme legibility; spread across 8 levels.
    const LEVELS: &[f64] = &[0.65, 0.48, 0.78, 0.40, 0.84, 0.56, 0.72, 0.44];
    hsl_to_hex(hue, sat, LEVELS[source_idx % LEVELS.len()])
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
