use leptos::prelude::*;
use crate::data::types::{DataSource, Modality, Resolution};

#[component]
pub fn Sidebar(
    sources: ReadSignal<Vec<DataSource>>,
    selected_sources: ReadSignal<Vec<String>>,
    on_source_toggle: Callback<String>,

    resolution: ReadSignal<Resolution>,
    on_resolution: Callback<Resolution>,

    selected_modalities: ReadSignal<Vec<Modality>>,
    on_modality_toggle: Callback<Modality>,

    date_from: ReadSignal<String>,
    date_to: ReadSignal<String>,
    on_date_from: Callback<String>,
    on_date_to: Callback<String>,

    on_preset: Callback<&'static str>,
) -> impl IntoView {
    view! {
        <aside>
            // ── Data Sources ──
            <div class="control-group">
                <label class="section-label">"Locations"</label>
                <div class="source-list">
                    {move || sources.get().into_iter().map(|src| {
                        let sel = selected_sources.get().contains(&src.id);
                        let id = src.id.clone();
                        let on_source_toggle = on_source_toggle.clone();
                        view! {
                            <div class=if sel { "source-item selected" } else { "source-item" }
                                 on:click=move |_| on_source_toggle.run(id.clone())>
                                <span class="source-dot"
                                      style=format!("background:{}", src.color)/>
                                <div>
                                    <div class="source-name">{src.name.clone()}</div>
                                    <div class="source-dates">
                                        {src.earliest.format("%Y").to_string()}
                                        " – "
                                        {src.latest.format("%Y").to_string()}
                                    </div>
                                </div>
                            </div>
                        }
                    }).collect_view()}
                </div>
            </div>

            // ── Modalities ──
            <div class="control-group">
                <label class="section-label">"Modalities"</label>
                <div class="btn-group">
                    {Modality::all().iter().map(|&m| {
                        let on_modality_toggle = on_modality_toggle.clone();
                        view! {
                            <button
                                class=move || if selected_modalities.get().contains(&m)
                                    { "active" } else { "" }
                                on:click=move |_| on_modality_toggle.run(m)>
                                {m.label()}
                            </button>
                        }
                    }).collect_view()}
                </div>
            </div>

            // ── Time Resolution ──
            <div class="control-group">
                <label class="section-label">"Resolution"</label>
                <div class="btn-group">
                    {[Resolution::Hour, Resolution::Day, Resolution::Week, Resolution::Month]
                        .into_iter().map(|r| {
                        let on_resolution = on_resolution.clone();
                        view! {
                            <button
                                class=move || if resolution.get() == r { "active" } else { "" }
                                on:click=move |_| on_resolution.run(r)>
                                {r.label()}
                            </button>
                        }
                    }).collect_view()}
                </div>
            </div>

            // ── Time Window ──
            <div class="control-group">
                <label class="section-label">"Presets"</label>
                <div class="btn-group">
                    <button on:click={
                        let on_preset = on_preset.clone();
                        move |_| on_preset.run("all")
                    }>"All dates"</button>
                    <button on:click={
                        let on_preset = on_preset.clone();
                        move |_| on_preset.run("winter")
                    }>"Winter"</button>
                    <button on:click={
                        let on_preset = on_preset.clone();
                        move |_| on_preset.run("summer")
                    }>"Summer"</button>
                </div>
            </div>

            // ── Custom Date Range ──
            <div class="control-group">
                <label class="section-label">"Custom range"</label>
                <div class="date-range">
                    <input type="date"
                           prop:value=move || date_from.get()
                           on:input=move |e| {
                               on_date_from.run(event_target_value(&e));
                           }/>
                    <input type="date"
                           prop:value=move || date_to.get()
                           on:input=move |e| {
                               on_date_to.run(event_target_value(&e));
                           }/>
                </div>
            </div>
        </aside>
    }
}
