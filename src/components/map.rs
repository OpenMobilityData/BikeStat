use leptos::prelude::*;
use crate::data::types::DataSource;

/// Equirectangular projection of (lat, lon) into SVG coordinates.
fn project(lat: f64, lon: f64,
           lat0: f64, lon0: f64, lat1: f64, lon1: f64,
           w: f64, h: f64) -> (f64, f64) {
    let x = (lon - lon0) / (lon1 - lon0) * w;
    let y = (1.0 - (lat - lat0) / (lat1 - lat0)) * h;
    (x, y)
}

/// Compute a padded bounding box from a list of sources.
/// Falls back to the Montreal region if no sources are loaded.
fn bounding_box(sources: &[DataSource]) -> (f64, f64, f64, f64) {
    if sources.is_empty() {
        // Montreal default
        return (45.40, 45.65, -73.98, -73.47);
    }
    let lats: Vec<f64> = sources.iter().map(|s| s.location.lat).collect();
    let lons: Vec<f64> = sources.iter().map(|s| s.location.lon).collect();
    let lat0 = lats.iter().cloned().fold(f64::INFINITY, f64::min);
    let lat1 = lats.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let lon0 = lons.iter().cloned().fold(f64::INFINITY, f64::min);
    let lon1 = lons.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    // 20% padding on each side, minimum span of 0.02°
    let lat_pad = (lat1 - lat0).max(0.02) * 0.25;
    let lon_pad = (lon1 - lon0).max(0.02) * 0.25;
    (lat0 - lat_pad, lat1 + lat_pad, lon0 - lon_pad, lon1 + lon_pad)
}

#[component]
pub fn SourceMap(
    sources: ReadSignal<Vec<DataSource>>,
    selected: ReadSignal<Vec<String>>,
    on_toggle: Callback<String>,
) -> impl IntoView {
    let (w, h) = (800.0_f64, 220.0_f64);

    view! {
        <div class="map-container">
            <svg viewBox=format!("0 0 {w} {h}") preserveAspectRatio="xMidYMid meet"
                 style="width:100%;height:100%;background:#0a1628">

                {move || {
                    let srcs = sources.get();
                    let sel  = selected.get();
                    let (lat0, lat1, lon0, lon1) = bounding_box(&srcs);

                    // Background
                    let bg = view! {
                        <rect x="0" y="0" width=w height=h fill="#0a1628"/>
                        <text x="50%" y="14" text-anchor="middle"
                              fill="#2a3a5c" font-size="10" font-family="system-ui">
                            {if srcs.is_empty() {
                                "Loading stations…"
                            } else {
                                "Click a marker to select / deselect"
                            }}
                        </text>
                    };

                    let markers = srcs.into_iter().map(|src| {
                        let (px, py) = project(
                            src.location.lat, src.location.lon,
                            lat0, lon0, lat1, lon1, w, h,
                        );
                        let is_sel   = sel.contains(&src.id);
                        let ring     = if is_sel { src.color.clone() } else { "#4a6fa0".into() };
                        let fill     = if is_sel { src.color.clone() } else { "#1a2a4a".into() };
                        let id       = src.id.clone();
                        let on_toggle = on_toggle.clone();
                        view! {
                            <g style="cursor:pointer"
                               on:click=move |_| on_toggle.run(id.clone())>
                                <circle cx=px cy=py r="7"
                                        fill=fill stroke=ring.clone() stroke-width="2"/>
                                <circle cx=px cy=py r="3" fill=ring/>
                                <text x=px y=(py + 16.0) text-anchor="middle"
                                      fill="#c0c8d8" font-size="8" font-family="system-ui">
                                    {src.name.clone()}
                                </text>
                            </g>
                        }
                    }).collect_view();

                    view! { <g>{bg}{markers}</g> }
                }}
            </svg>
        </div>
    }
}
