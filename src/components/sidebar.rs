use chrono::NaiveDate;
use leptos::prelude::*;
use crate::data::types::{DataSource, DayKind, Modality, Resolution, ViewMode};
use crate::i18n::Lang;

#[component]
pub fn Sidebar(
    sources: ReadSignal<Vec<DataSource>>,
    selected_sources: ReadSignal<Vec<String>>,
    on_source_toggle: Callback<String>,

    resolution: ReadSignal<Resolution>,
    on_resolution: Callback<Resolution>,

    selected_modalities: ReadSignal<Vec<Modality>>,
    on_modality_toggle: Callback<Modality>,

    date_from: ReadSignal<NaiveDate>,
    date_to: ReadSignal<NaiveDate>,
    on_date_from: Callback<NaiveDate>,
    on_date_to: Callback<NaiveDate>,

    date_presets: ReadSignal<Vec<(String, NaiveDate, NaiveDate, i64, Option<Resolution>)>>,
    on_date_preset: Callback<(NaiveDate, NaiveDate, Option<Resolution>)>,

    view_mode: ReadSignal<ViewMode>,
    on_year_on_year: Callback<()>,
    on_winter_on_winter: Callback<()>,

    selected_day_kinds: ReadSignal<Vec<DayKind>>,
    on_day_kind_toggle: Callback<DayKind>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context not provided");
    view! {
        <aside>
            // ── Data Sources ──
            <div class="control-group">
                <label class="section-label">{move || lang.get().t().locations}</label>
                <div class="source-list">
                    {move || {
                        // Collect consecutive sources that share the same group key
                        // into clusters; standalone sources form single-item clusters.
                        let srcs = sources.get();
                        let mut clusters: Vec<Vec<DataSource>> = Vec::new();
                        for src in srcs {
                            let same = clusters.last()
                                .and_then(|c: &Vec<_>| c[0].group.as_ref())
                                .zip(src.group.as_ref())
                                .map(|(a, b)| a == b)
                                .unwrap_or(false);
                            if same { clusters.last_mut().unwrap().push(src); }
                            else    { clusters.push(vec![src]); }
                        }

                        clusters.into_iter().map(|cluster| {
                            let on_toggle = on_source_toggle.clone();
                            if cluster.len() == 1 {
                                let src = cluster.into_iter().next().unwrap();
                                let sel = selected_sources.get().contains(&src.id);
                                let id  = src.id.clone();
                                view! {
                                    <div
                                        class=if sel { "source-item selected" } else { "source-item" }
                                        on:click=move |_| on_toggle.run(id.clone())>
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
                                }.into_any()
                            } else {
                                view! {
                                    <div class="source-group">
                                        {cluster.into_iter().map(|src| {
                                            let sel = selected_sources.get().contains(&src.id);
                                            let id  = src.id.clone();
                                            let on_toggle = on_toggle.clone();
                                            view! {
                                                <div
                                                    class=if sel { "source-item selected" } else { "source-item" }
                                                    on:click=move |_| on_toggle.run(id.clone())>
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
                                }.into_any()
                            }
                        }).collect_view()
                    }}
                </div>
            </div>

            // ── Modalities ──
            <div class="control-group">
                <label class="section-label">{move || lang.get().t().modalities}</label>
                <div class="btn-group">
                    {Modality::all().iter().map(|&m| {
                        let on_modality_toggle = on_modality_toggle.clone();
                        view! {
                            <button
                                class=move || if selected_modalities.get().contains(&m)
                                    { "active" } else { "" }
                                on:click=move |_| on_modality_toggle.run(m)>
                                {move || m.label(lang.get())}
                            </button>
                        }
                    }).collect_view()}
                </div>
            </div>

            // ── Time Resolution ──
            <div class="control-group">
                <label class="section-label">{move || lang.get().t().resolution}</label>
                <div class="btn-group">
                    {[Resolution::Hour, Resolution::Day, Resolution::Week, Resolution::Month]
                        .into_iter().map(|r| {
                        let on_resolution = on_resolution.clone();
                        // Daily Averaging is intrinsically hourly — lock the
                        // selector to Hour while it's active so the buttons'
                        // disabled state mirrors the chart behavior.
                        let is_daily_avg = move || view_mode.get() == ViewMode::DailyAveraging;
                        view! {
                            <button
                                disabled=is_daily_avg
                                class=move || if resolution.get() == r { "active" } else { "" }
                                on:click=move |_| on_resolution.run(r)>
                                {move || r.label(lang.get())}
                            </button>
                        }
                    }).collect_view()}
                </div>
            </div>

            // ── Time Window ──
            <div class="control-group">
                <label class="section-label">{move || lang.get().t().date_range}</label>
                <div class="btn-group">
                    {move || {
                        // A preset is disabled if its day-span is shorter than
                        // what the current resolution can summarize without
                        // ending up with all-partial buckets that get trimmed.
                        let res = resolution.get();
                        let l   = lang.get();
                        let min_days: i64 = match res {
                            Resolution::Hour | Resolution::Day => 0,
                            Resolution::Week  => 14,
                            Resolution::Month => 60,
                        };
                        let res_label = res.label(l).to_lowercase();
                        date_presets.get().into_iter().map(|(label, from, to, days, force_res)| {
                            let on_date_preset = on_date_preset.clone();
                            let disabled = days < min_days;
                            let title = if disabled {
                                format!("{} {}", l.t().range_too_short, res_label)
                            } else {
                                String::new()
                            };
                            view! {
                                <button
                                    disabled=disabled
                                    title=title
                                    on:click=move |_| on_date_preset.run((from, to, force_res))>
                                    {label}
                                </button>
                            }
                        }).collect_view()
                    }}
                    <button
                        class=move || if view_mode.get() == ViewMode::YearOnYear { "active" } else { "" }
                        on:click=move |_| on_year_on_year.run(())>
                        {move || lang.get().t().year_on_year}
                    </button>
                    <button
                        class=move || if view_mode.get() == ViewMode::WinterOnWinter { "active" } else { "" }
                        on:click=move |_| on_winter_on_winter.run(())>
                        {move || lang.get().t().winter_on_winter}
                    </button>
                </div>
            </div>

            // ── Daily Averaging ──
            <div class="control-group">
                <label class="section-label">{move || lang.get().t().daily_averaging}</label>
                <div class="btn-group">
                    {[DayKind::Weekday, DayKind::Weekend].into_iter().map(|k| {
                        let on_day_kind_toggle = on_day_kind_toggle.clone();
                        view! {
                            <button
                                class=move || if selected_day_kinds.get().contains(&k)
                                    { "active" } else { "" }
                                on:click=move |_| on_day_kind_toggle.run(k)>
                                {move || k.label(lang.get())}
                            </button>
                        }
                    }).collect_view()}
                </div>
            </div>

            // ── Custom Date Range ──
            <div class="control-group">
                <label class="section-label">{move || lang.get().t().custom_range}</label>
                <div class="date-range">
                    <input type="date"
                           prop:value=move || date_from.get().format("%Y-%m-%d").to_string()
                           on:input=move |e| {
                               if let Ok(d) = NaiveDate::parse_from_str(
                                   &event_target_value(&e), "%Y-%m-%d") {
                                   on_date_from.run(d);
                               }
                           }/>
                    <input type="date"
                           prop:value=move || date_to.get().format("%Y-%m-%d").to_string()
                           on:input=move |e| {
                               if let Ok(d) = NaiveDate::parse_from_str(
                                   &event_target_value(&e), "%Y-%m-%d") {
                                   on_date_to.run(d);
                               }
                           }/>
                </div>
            </div>
        </aside>
    }
}
