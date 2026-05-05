mod data;
mod components;

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

use data::loader;
use data::types::{CountRecord, DataSource, Modality, Resolution};
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
    // ── Core state ──
    let (sources, set_sources) = signal::<Vec<DataSource>>(vec![]);
    let (records, set_records) = signal::<Vec<CountRecord>>(vec![]);

    // UI state
    let (selected_srcs, set_selected_srcs) = signal::<Vec<String>>(vec![]);
    let (selected_mods, set_selected_mods) = signal::<Vec<Modality>>(vec![Modality::Bikes]);
    let (resolution,    set_resolution)    = signal(Resolution::Day);
    let (load_status,   set_load_status)   = signal::<LoadStatus>(LoadStatus::Loading);

    let now = Utc::now();
    let (date_from, set_date_from) = signal(format!("{}-01-01", now.year()));
    let (date_to,   set_date_to)   = signal(format!("{}-12-31", now.year()));

    // ── Fetch Montreal data on startup ──
    spawn_local(async move {
        set_load_status.set(LoadStatus::Loading);
        match loader::fetch_montreal_cyclistes().await {
            Ok((new_sources, new_records)) => {
                // Set date range to span the full dataset
                if let (Some(first), Some(last)) = (
                    new_records.iter().map(|r| r.timestamp).min(),
                    new_records.iter().map(|r| r.timestamp).max(),
                ) {
                    set_date_from.set(first.format("%Y-%m-%d").to_string());
                    set_date_to.set(last.format("%Y-%m-%d").to_string());
                }
                set_sources.update(|s| s.extend(new_sources));
                set_records.update(|r| r.extend(new_records));
                set_load_status.set(LoadStatus::Ready);
            }
            Err(e) => set_load_status.set(LoadStatus::Error(e)),
        }
    });

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
            } else {
                v.push(m);
            }
        });
    });

    let on_preset = Callback::new(move |preset: &'static str| {
        let y = Utc::now().year();
        match preset {
            "winter" => {
                set_date_from.set(format!("{}-11-01", y - 1));
                set_date_to.set(format!("{}-03-31", y));
            }
            "summer" => {
                set_date_from.set(format!("{}-05-01", y));
                set_date_to.set(format!("{}-09-30", y));
            }
            _ => {
                // "all" — span the loaded data
                let recs = records.get_untracked();
                if let (Some(first), Some(last)) = (
                    recs.iter().map(|r| r.timestamp).min(),
                    recs.iter().map(|r| r.timestamp).max(),
                ) {
                    set_date_from.set(first.format("%Y-%m-%d").to_string());
                    set_date_to.set(last.format("%Y-%m-%d").to_string());
                }
            }
        }
    });

    // ── Derived chart series ──
    let chart_series = move || -> Vec<Series> {
        let recs     = records.get();
        let mods     = selected_mods.get();
        let res      = resolution.get();
        let srcs     = selected_srcs.get();
        let all_srcs = sources.get();
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
                if let Some(meta) = all_srcs.iter().find(|s| &s.id == src_id) {
                    if !meta.modalities.contains(modality) { continue; }
                    let mut pts = loader::aggregate(&recs, *modality, res, Some(src_id));
                    if let Some(f) = from_dt { pts.retain(|(dt, _)| *dt >= f); }
                    if let Some(t) = to_dt   { pts.retain(|(dt, _)| *dt <= t); }
                    if !pts.is_empty() {
                        out.push(Series {
                            label: format!("{} – {}", meta.name, modality.label()),
                            color: meta.color.clone(),
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

    view! {
        <div id="app">
            <header>
                <h1>"NoBikes"</h1>
                <span class="subtitle">"Traffic Count Explorer"</span>
                <span class="load-status">{move || match load_status.get() {
                    LoadStatus::Loading => "⏳ Loading data…".to_string(),
                    LoadStatus::Ready   => String::new(),
                    LoadStatus::Error(e) => format!("⚠ {}", e),
                }}</span>
            </header>

            <Sidebar
                sources=sources
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
            />

            <main>
                <Chart series=chart_sig />
                <SourceMap
                    sources=sources
                    selected=selected_srcs
                    on_toggle=on_map_toggle
                />
            </main>
        </div>
    }
}

#[derive(Clone, PartialEq)]
enum LoadStatus {
    Loading,
    Ready,
    Error(String),
}
