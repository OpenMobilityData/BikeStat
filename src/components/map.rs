use leptos::prelude::*;
use crate::data::types::DataSource;

const TILE_SIZE: f64 = 256.0;
/// Width target used to pick the zoom level. Actual container may be wider.
const ZOOM_TARGET_W: f64 = 1100.0;
const VIEWPORT_H: f64 = 220.0;
/// After picking integer zoom, markers are scaled up to fill this fraction
/// of the constraining viewport dimension (CSS transform on .map-content).
const FIT_FRACTION: f64 = 0.70;
/// Cap on the additional scale; beyond this tiles get noticeably blurry.
const MAX_SCALE: f64 = 2.5;
/// Render tiles to cover up to this *scaled* pixel width either side of centre.
const ASSUMED_MAX_VIEWPORT_W: f64 = 3200.0;

/// (lat, lon) → fractional Web Mercator tile coordinates at a given zoom.
fn lat_lon_to_tile(lat: f64, lon: f64, zoom: u32) -> (f64, f64) {
    let n = (1u64 << zoom) as f64;
    let x = (lon + 180.0) / 360.0 * n;
    let y = (1.0 - lat.to_radians().tan().asinh() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}

/// Largest zoom (≤18) at which the bbox fits within (w, h) pixels.
fn pick_zoom(lat0: f64, lat1: f64, lon0: f64, lon1: f64, w: f64, h: f64) -> u32 {
    for z in (0..=18u32).rev() {
        let (x0, y_top) = lat_lon_to_tile(lat1, lon0, z);
        let (x1, y_bot) = lat_lon_to_tile(lat0, lon1, z);
        if (x1 - x0) * TILE_SIZE <= w && (y_bot - y_top) * TILE_SIZE <= h {
            return z;
        }
    }
    0
}

/// One marker per location group: keep ungrouped sources as-is, and for grouped
/// sources keep only the representative (id == group key, which is the total).
fn marker_sources(srcs: &[DataSource]) -> Vec<DataSource> {
    srcs.iter()
        .filter(|s| match &s.group {
            None => true,
            Some(g) => &s.id == g,
        })
        .cloned()
        .collect()
}

/// The total source's name carries a trailing qualifier that's redundant once
/// the marker represents the whole group.
fn strip_total_suffix(name: &str) -> &str {
    name.strip_suffix(" — Total")
        .or_else(|| name.strip_suffix(" (Total)"))
        .unwrap_or(name)
}

/// Unpadded bbox enclosing all markers. A minimum span guards against the
/// degenerate single-marker case so pick_zoom has something to fit against.
fn marker_bbox(sources: &[DataSource]) -> (f64, f64, f64, f64) {
    if sources.is_empty() {
        return (45.40, 45.65, -73.98, -73.47);
    }
    const MIN_SPAN: f64 = 0.005;
    let lats: Vec<f64> = sources.iter().map(|s| s.location.lat).collect();
    let lons: Vec<f64> = sources.iter().map(|s| s.location.lon).collect();
    let lat0 = lats.iter().cloned().fold(f64::INFINITY, f64::min);
    let lat1 = lats.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let lon0 = lons.iter().cloned().fold(f64::INFINITY, f64::min);
    let lon1 = lons.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let cy = (lat0 + lat1) / 2.0;
    let cx = (lon0 + lon1) / 2.0;
    let lat_half = ((lat1 - lat0).max(MIN_SPAN)) / 2.0;
    let lon_half = ((lon1 - lon0).max(MIN_SPAN)) / 2.0;
    (cy - lat_half, cy + lat_half, cx - lon_half, cx + lon_half)
}

#[component]
pub fn SourceMap(
    sources: ReadSignal<Vec<DataSource>>,
    selected: ReadSignal<Vec<String>>,
    on_toggle: Callback<String>,
) -> impl IntoView {
    view! {
        <div class="map-container">
            {move || {
                let all_srcs = sources.get();
                let srcs     = marker_sources(&all_srcs);
                let sel      = selected.get();
                let is_empty = all_srcs.is_empty();

                let (lat0, lat1, lon0, lon1) = marker_bbox(&srcs);
                let zoom = pick_zoom(lat0, lat1, lon0, lon1, ZOOM_TARGET_W, VIEWPORT_H);
                let (cx_tile, cy_tile) = lat_lon_to_tile(
                    (lat0 + lat1) / 2.0, (lon0 + lon1) / 2.0, zoom,
                );

                // Scale up so markers fill ~FIT_FRACTION of the constraining viewport
                // dimension, simulating fractional zoom on top of the integer tile zoom.
                let (mx0, my0) = lat_lon_to_tile(lat1, lon0, zoom);
                let (mx1, my1) = lat_lon_to_tile(lat0, lon1, zoom);
                let marker_w_px = ((mx1 - mx0) * TILE_SIZE).max(1.0);
                let marker_h_px = ((my1 - my0) * TILE_SIZE).max(1.0);
                let scale_w = ZOOM_TARGET_W * FIT_FRACTION / marker_w_px;
                let scale_h = VIEWPORT_H     * FIT_FRACTION / marker_h_px;
                let scale = scale_w.min(scale_h).clamp(1.0, MAX_SCALE);

                // Tile grid covers the visible viewport in *scaled* pixels.
                let half_w_tiles = (ASSUMED_MAX_VIEWPORT_W / 2.0 / scale / TILE_SIZE) + 1.0;
                let half_h_tiles = (VIEWPORT_H            / 2.0 / scale / TILE_SIZE) + 1.0;
                let tx_min = (cx_tile - half_w_tiles).floor() as i64;
                let tx_max = (cx_tile + half_w_tiles).floor() as i64;
                let ty_min = (cy_tile - half_h_tiles).floor() as i64;
                let ty_max = (cy_tile + half_h_tiles).floor() as i64;
                let n_tiles: i64 = 1i64 << zoom;

                let tile_views = (tx_min..=tx_max)
                    .flat_map(|tx| (ty_min..=ty_max).map(move |ty| (tx, ty)))
                    .filter(|&(_, ty)| ty >= 0 && ty < n_tiles)
                    .map(|(tx, ty)| {
                        let tx_wrapped = ((tx % n_tiles) + n_tiles) % n_tiles;
                        let off_x = (tx as f64 - cx_tile) * TILE_SIZE;
                        let off_y = (ty as f64 - cy_tile) * TILE_SIZE;
                        let url = format!(
                            "https://a.basemaps.cartocdn.com/dark_nolabels/{}/{}/{}@2x.png",
                            zoom, tx_wrapped, ty,
                        );
                        let style = format!(
                            "left: calc(50% + {:.0}px); top: calc(50% + {:.0}px);",
                            off_x, off_y,
                        );
                        view! {
                            <img class="map-tile" src=url style=style draggable="false"/>
                        }
                    })
                    .collect_view();

                let markers = srcs.into_iter().map(|src| {
                    let (sx, sy) = lat_lon_to_tile(src.location.lat, src.location.lon, zoom);
                    let off_x = (sx - cx_tile) * TILE_SIZE;
                    let off_y = (sy - cy_tile) * TILE_SIZE;
                    let is_sel = sel.contains(&src.id);
                    let dot_style = if is_sel {
                        format!("background:{0};border-color:{0};", src.color)
                    } else {
                        String::new()
                    };
                    // If the dot sits in the bottom portion of the viewport, flip the
                    // label above it so it doesn't get clipped by overflow:hidden.
                    let dot_screen_y = VIEWPORT_H / 2.0 + off_y * scale;
                    let label_above  = dot_screen_y > VIEWPORT_H - 36.0;
                    let class = match (is_sel, label_above) {
                        (true,  true)  => "map-marker selected label-above",
                        (true,  false) => "map-marker selected",
                        (false, true)  => "map-marker label-above",
                        (false, false) => "map-marker",
                    };
                    let id        = src.id.clone();
                    let on_toggle = on_toggle.clone();
                    let full_name = src.name.clone();
                    let label     = strip_total_suffix(&src.name).to_string();
                    let style = format!(
                        "left: calc(50% + {:.0}px); top: calc(50% + {:.0}px);",
                        off_x, off_y,
                    );
                    view! {
                        <div class=class style=style title=full_name
                             on:click=move |_| on_toggle.run(id.clone())>
                            <span class="marker-dot" style=dot_style></span>
                            <span class="marker-label">{label}</span>
                        </div>
                    }
                }).collect_view();

                let hint = if is_empty {
                    "Loading stations…"
                } else {
                    "Click a marker to select / deselect"
                };

                let content_style = format!("transform: scale({:.4});", scale);
                view! {
                    <div class="map-content" style=content_style>
                        <div class="map-tiles">{tile_views}</div>
                        {markers}
                    </div>
                    <div class="map-hint">{hint}</div>
                    <div class="map-attribution">"© OpenStreetMap © CARTO"</div>
                }
            }}
        </div>
    }
}
