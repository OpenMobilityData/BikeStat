use leptos::prelude::*;
use crate::data::types::DataSource;

fn project(lat: f64, lon: f64, lat0: f64, lon0: f64, lat1: f64, lon1: f64,
           w: f64, h: f64) -> (f64, f64) {
    let x = (lon - lon0) / (lon1 - lon0) * w;
    let y = (1.0 - (lat - lat0) / (lat1 - lat0)) * h;
    (x, y)
}

#[component]
pub fn SourceMap(
    sources: ReadSignal<Vec<DataSource>>,
    selected: ReadSignal<Vec<String>>,
    on_toggle: Callback<String>,
) -> impl IntoView {
    let lat0 = 49.20_f64;
    let lat1 = 49.35_f64;
    let lon0 = -123.25_f64;
    let lon1 = -123.00_f64;
    let (w, h) = (800.0_f64, 220.0_f64);

    view! {
        <div class="map-container">
            <svg viewBox=format!("0 0 {w} {h}") preserveAspectRatio="xMidYMid meet"
                 style="width:100%;height:100%;background:#0a1628">

                <rect x="0" y="0" width=w height=h fill="#0a1628"/>
                <text x="50%" y="14" text-anchor="middle"
                      fill="#2a3a5c" font-size="11" font-family="system-ui">
                    "Vancouver area — click a marker to toggle selection"
                </text>

                {move || {
                    let sel = selected.get();
                    sources.get().into_iter().map(|src| {
                        let (px, py) = project(
                            src.location.lat, src.location.lon,
                            lat0, lon0, lat1, lon1, w, h,
                        );
                        let is_sel = sel.contains(&src.id);
                        let ring_color = if is_sel { src.color.clone() } else { "#4a6fa0".into() };
                        let fill_color = if is_sel { src.color.clone() } else { "#1a2a4a".into() };
                        let id = src.id.clone();
                        let on_toggle = on_toggle.clone();
                        view! {
                            <g style="cursor:pointer"
                               on:click=move |_| on_toggle.run(id.clone())>
                                <circle cx=px cy=py r="10"
                                        fill=fill_color stroke=ring_color.clone() stroke-width="2"/>
                                <circle cx=px cy=py r="4" fill=ring_color/>
                                <text x=px y=(py + 18.0) text-anchor="middle"
                                      fill="#eaeaea" font-size="9" font-family="system-ui">
                                    {src.name.clone()}
                                </text>
                            </g>
                        }
                    }).collect_view()
                }}
            </svg>
        </div>
    }
}
