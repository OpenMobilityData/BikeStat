mod data;
mod components;

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use leptos::prelude::*;
use wasm_bindgen::prelude::*;

use data::sources::catalogue;
use data::types::{CountRecord, Modality, Resolution};
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
    let all_sources = catalogue();

    let (sources, _)           = signal(all_sources.clone());
    let (selected_srcs, set_selected_srcs) = signal::<Vec<String>>(vec![]);
    let (selected_mods, set_selected_mods) = signal::<Vec<Modality>>(vec![Modality::Bikes]);
    let (resolution, set_resolution)       = signal(Resolution::Day);
    let (records, _set_records)            = signal::<Vec<CountRecord>>(vec![]);

    let now = Utc::now();
    let (date_from, set_date_from) = signal(format!("{}-01-01", now.year()));
    let (date_to,   set_date_to)   = signal(format!("{}-12-31", now.year()));

    // ── Source toggle ──
    let toggle_source = move |id: String| {
        set_selected_srcs.update(|v| {
            if let Some(i) = v.iter().position(|s| s == &id) { v.remove(i); }
            else { v.push(id); }
        });
    };
    let on_source_toggle = Callback::new(toggle_source.clone());
    let on_map_toggle    = Callback::new(toggle_source);

    // ── Modality toggle ──
    let on_modality_toggle = Callback::new(move |m: Modality| {
        set_selected_mods.update(|v| {
            if let Some(i) = v.iter().position(|&x| x == m) {
                if v.len() > 1 { v.remove(i); }
            } else {
                v.push(m);
            }
        });
    });

    // ── Preset ──
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
                set_date_from.set(format!("{}-01-01", y));
                set_date_to.set(format!("{}-12-31", y));
            }
        }
    });

    // ── Derived chart series ──
    let chart_series = move || -> Vec<Series> {
        let recs     = records.get();
        let mods     = selected_mods.get();
        let res      = resolution.get();
        let srcs     = selected_srcs.get();
        let from_str = date_from.get();
        let to_str   = date_to.get();

        let from_dt = NaiveDate::parse_from_str(&from_str, "%Y-%m-%d")
            .ok().and_then(|d| d.and_hms_opt(0,0,0))
            .map(|ndt| Utc.from_utc_datetime(&ndt));
        let to_dt = NaiveDate::parse_from_str(&to_str, "%Y-%m-%d")
            .ok().and_then(|d| d.and_hms_opt(23,59,59))
            .map(|ndt| Utc.from_utc_datetime(&ndt));

        let mut out = vec![];
        for modality in &mods {
            for src_id in &srcs {
                if let Some(meta) = catalogue().into_iter().find(|s| &s.id == src_id) {
                    let mut pts = data::loader::aggregate(&recs, *modality, res, Some(src_id));
                    if let Some(f) = from_dt { pts.retain(|(dt, _)| *dt >= f); }
                    if let Some(t) = to_dt   { pts.retain(|(dt, _)| *dt <= t); }
                    if !pts.is_empty() {
                        out.push(Series {
                            label: format!("{} – {}", meta.name, modality.label()),
                            color: modality.color().to_string(),
                            points: pts,
                        });
                    }
                }
            }
        }
        out
    };

    // Wrap in a signal so Chart sees a ReadSignal
    let (chart_sig, set_chart_sig) = signal::<Vec<Series>>(vec![]);
    Effect::new(move |_| set_chart_sig.set(chart_series()));

    view! {
        <div id="app">
            <header>
                <h1>"NoBikes"</h1>
                <span class="subtitle">"Traffic Count Explorer"</span>
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
