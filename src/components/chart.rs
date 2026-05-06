use chrono::{DateTime, Utc};
use chrono_tz::America::Montreal as MontrealTz;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::data::types::Resolution;

#[derive(Clone, PartialEq)]
pub struct Series {
    pub label: String,
    pub color: String,
    /// SVG `stroke-dasharray` value; empty string means solid.
    pub dash: String,
    pub points: Vec<(DateTime<Utc>, f64)>,
}

fn series_stats(points: &[(DateTime<Utc>, f64)]) -> String {
    if points.is_empty() { return String::new(); }
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0_f64;
    for (_, v) in points {
        if *v < min { min = *v; }
        if *v > max { max = *v; }
        sum += *v;
    }
    format!("min {}  max {}  total {}", fmt_count(min), fmt_count(max), fmt_count(sum))
}

fn fmt_count(v: f64) -> String {
    let n = v.round() as i64;
    let mag = n.unsigned_abs().to_string();
    let bytes = mag.as_bytes();
    let mut out = String::new();
    if n < 0 { out.push('-'); }
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 { out.push(','); }
        out.push(*b as char);
    }
    out
}

/// Layout + data-range parameters needed to map between data coordinates
/// (timestamp / value) and SVG user coordinates.  Shared between the
/// rendering closure and the hover handler so they agree on positions.
#[derive(Clone)]
struct ChartGeom {
    x_min: f64,
    y_min: f64,
    x_span: f64,
    y_span: f64,
    pad_l: f64,
    pad_t: f64,
    w: f64,
    h: f64,
}

impl ChartGeom {
    fn to_x(&self, ts: i64) -> f64 {
        self.pad_l + (ts as f64 - self.x_min) / self.x_span * self.w
    }
    fn to_y(&self, v: f64) -> f64 {
        self.pad_t + self.h - (v - self.y_min) / self.y_span * self.h
    }
}

fn compute_geom(
    s: &[Series],
    xr: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pad_l: f64,
    pad_t: f64,
    w: f64,
    h: f64,
) -> Option<ChartGeom> {
    if (s.is_empty() || s.iter().all(|ser| ser.points.is_empty())) && xr.is_none() {
        return None;
    }
    let (x_min, x_max) = match xr {
        Some((lo, hi)) => (lo.timestamp() as f64, hi.timestamp() as f64),
        None => {
            let xs: Vec<i64> = s.iter()
                .flat_map(|ser| ser.points.iter().map(|(t, _)| t.timestamp()))
                .collect();
            (*xs.iter().min().unwrap() as f64, *xs.iter().max().unwrap() as f64)
        }
    };
    let y_min = 0.0_f64;
    let y_max = s.iter()
        .flat_map(|ser| ser.points.iter().map(|(_, v)| *v))
        .fold(0.0_f64, f64::max) * 1.1;
    Some(ChartGeom {
        x_min,
        y_min,
        x_span: (x_max - x_min).max(1.0),
        y_span: (y_max - y_min).max(1.0),
        pad_l,
        pad_t,
        w,
        h,
    })
}

/// Hover state — set on every mousemove over the plot area, cleared on leave.
#[derive(Clone)]
struct HoverInfo {
    crosshair_x: f64,    // SVG x for the vertical crosshair line
    client_x: f64,       // viewport pixels, for tooltip positioning
    client_y: f64,
    flip_x: bool,        // tooltip to the LEFT of cursor (near right edge)
    flip_y: bool,        // tooltip ABOVE cursor (near bottom edge)
    rows: Vec<HoverRow>,
}

#[derive(Clone)]
struct HoverRow {
    color: String,
    label: String,
    timestamp: DateTime<Utc>,
    value: f64,
    point_x: f64,        // SVG coords for the dot
    point_y: f64,
}

/// Format the tooltip's lead-in date. For Hour resolution, surface the
/// Montreal local time (so tooltips read like commute times instead of
/// UTC offsets). For coarser resolutions, drop the time entirely — the
/// bucket's hour is just an artifact of UTC-midnight bucketing.
fn format_hover_date(ts: DateTime<Utc>, res: Resolution) -> String {
    match res {
        Resolution::Hour => ts.with_timezone(&MontrealTz)
            .format("%Y-%m-%d %H:%M %Z").to_string(),
        Resolution::Day | Resolution::Week | Resolution::Month =>
            ts.format("%Y-%m-%d").to_string(),
    }
}

/// Maximum allowed gap (in seconds) between consecutive bucketed points
/// before the line is broken. Set to 2× the minimum bucket spacing so a
/// single missing bucket reliably triggers a break. February's 28 days is
/// the floor for Month so e.g. Apr→Jun (one missing May) still breaks.
fn gap_threshold_secs(res: Resolution) -> i64 {
    match res {
        Resolution::Hour  => 2 * 3600,
        Resolution::Day   => 2 * 86_400,
        Resolution::Week  => 2 * 7 * 86_400,
        Resolution::Month => 2 * 28 * 86_400,
    }
}

#[component]
pub fn Chart(
    series: ReadSignal<Vec<Series>>,
    /// When Some, fixes the X-axis to the given (start, end) range instead of
    /// auto-deriving it from the points. Used by Year-on-Year so the axis
    /// always shows a full 12-month span even with partial data.
    x_range: ReadSignal<Option<(DateTime<Utc>, DateTime<Utc>)>>,
    /// Current bucket resolution; used to format the tooltip date.
    resolution: ReadSignal<Resolution>,
) -> impl IntoView {
    let view_box = "0 0 900 400";
    let pad_l = 60.0_f64;
    let pad_r = 20.0_f64;
    let pad_t = 20.0_f64;
    let pad_b = 50.0_f64;
    let w = 900.0_f64 - pad_l - pad_r;
    let h = 400.0_f64 - pad_t - pad_b;

    let (hover, set_hover) = signal::<Option<HoverInfo>>(None);

    // Clear the (position: fixed) tooltip when the user scrolls the page or
    // taps anywhere outside the chart (e.g. opens the Filters panel). The
    // chart's own touch handler calls stop_propagation, so taps on the chart
    // itself don't reach this listener.
    let _ = leptos::prelude::window_event_listener(leptos::ev::scroll, move |_| set_hover.set(None));
    let _ = leptos::prelude::window_event_listener(leptos::ev::touchstart, move |_| set_hover.set(None));

    let derived = move || {
        let s = series.get();
        let xr = x_range.get();
        let res = resolution.get();
        let g = compute_geom(&s, xr, pad_l, pad_t, w, h)?;
        let y_max = g.y_min + g.y_span;
        let base_y = g.to_y(g.y_min);
        let gap_threshold = gap_threshold_secs(res);

        // Build per-series line and area paths, breaking the path whenever
        // two consecutive points are spaced further apart than gap_threshold
        // (i.e. data is missing). Each contiguous run becomes its own
        // sub-path; the area sub-path is closed back down to the baseline.
        let paths: Vec<(String, String, String, String)> = s.iter().map(|ser| {
            if ser.points.is_empty() {
                return (ser.color.clone(), ser.dash.clone(), String::new(), String::new());
            }
            let mut line_d = String::new();
            let mut area_d = String::new();
            let mut prev_ts: Option<i64> = None;
            let mut seg_first_x: Option<f64> = None;
            let mut seg_last_x: f64 = 0.0;
            for (dt, v) in &ser.points {
                let ts = dt.timestamp();
                let x = g.to_x(ts);
                let y = g.to_y(*v);
                let new_segment = match prev_ts {
                    None => true,
                    Some(p) => ts - p > gap_threshold,
                };
                if new_segment {
                    if let Some(fx) = seg_first_x {
                        // close previous area sub-path
                        area_d.push_str(&format!(" L {:.1},{:.1} L {:.1},{:.1} Z",
                            seg_last_x, base_y, fx, base_y));
                    }
                    if !line_d.is_empty() { line_d.push(' '); }
                    line_d.push_str(&format!("M {:.1},{:.1}", x, y));
                    if !area_d.is_empty() { area_d.push(' '); }
                    area_d.push_str(&format!("M {:.1},{:.1}", x, y));
                    seg_first_x = Some(x);
                } else {
                    line_d.push_str(&format!(" L {:.1},{:.1}", x, y));
                    area_d.push_str(&format!(" L {:.1},{:.1}", x, y));
                }
                seg_last_x = x;
                prev_ts = Some(ts);
            }
            if let Some(fx) = seg_first_x {
                area_d.push_str(&format!(" L {:.1},{:.1} L {:.1},{:.1} Z",
                    seg_last_x, base_y, fx, base_y));
            }
            (ser.color.clone(), ser.dash.clone(), line_d, area_d)
        }).collect();

        // Y-axis ticks
        let tick_count = 5;
        let y_ticks: Vec<(f64, String)> = (0..=tick_count).map(|i| {
            let v = g.y_min + (y_max - g.y_min) * i as f64 / tick_count as f64;
            (g.to_y(v), format!("{:.0}", v))
        }).collect();

        // X-axis ticks (up to 8)
        let max_points = s.iter().map(|ser| ser.points.len()).max().unwrap_or(0);
        let x_tick_n = 7.min(max_points.max(if xr.is_some() { 7 } else { 0 }));
        let x_ticks: Vec<(f64, String)> = if x_tick_n == 0 { vec![] } else {
            (0..=x_tick_n).map(|i| {
                let ts = (g.x_min + g.x_span * i as f64 / x_tick_n as f64) as i64;
                let x = g.to_x(ts);
                let label = DateTime::from_timestamp(ts, 0)
                    .map(|dt: DateTime<Utc>| dt.format("%b %d").to_string())
                    .unwrap_or_default();
                (x, label)
            }).collect()
        };

        Some((paths, y_ticks, x_ticks))
    };

    // Compute hover state for a pointer at given viewport coordinates over the
    // overlay rect.  Shared by mouse and touch handlers so both input modes
    // produce identical crosshair + tooltip behavior.
    let compute_hover = move |client_x: f64, client_y: f64, rect: web_sys::DomRect, force_flip_y: bool| -> Option<HoverInfo> {
        let s = series.get();
        let xr = x_range.get();
        let g = compute_geom(&s, xr, pad_l, pad_t, w, h)?;

        let pointer_x_in_overlay = client_x - rect.left();
        let pointer_y_in_overlay = client_y - rect.top();
        let frac_x = (pointer_x_in_overlay / rect.width().max(1.0)).clamp(0.0, 1.0);
        let frac_y = (pointer_y_in_overlay / rect.height().max(1.0)).clamp(0.0, 1.0);

        // The overlay rect is exactly the plot area, so its fractional
        // coordinates map directly to SVG user space within (pad_l..pad_l+w,
        // pad_t..pad_t+h).
        let cursor_svg_x = g.pad_l + frac_x * g.w;
        let cursor_svg_y = g.pad_t + frac_y * g.h;

        // Anchor the snap to the globally-nearest point across all series in
        // SVG (pixel) space.  Pure time-based snapping struggles with sharp
        // peaks at high resolution: at hour resolution the chart can pack
        // several hours per pixel, so a peak's adjacent low-value hour can
        // win the "nearest in time" race even when you're aiming squarely
        // at the spike.  Euclidean distance on the rendered points means
        // moving the cursor up toward a tall spike snaps onto it.
        let mut anchor_ts: Option<DateTime<Utc>> = None;
        let mut best_d2 = f64::INFINITY;
        for ser in s.iter() {
            for (t, v) in &ser.points {
                let px = g.to_x(t.timestamp());
                let py = g.to_y(*v);
                let d2 = (px - cursor_svg_x).powi(2) + (py - cursor_svg_y).powi(2);
                if d2 < best_d2 {
                    best_d2 = d2;
                    anchor_ts = Some(*t);
                }
            }
        }

        let anchor_ts = anchor_ts?;
        let anchor_secs = anchor_ts.timestamp() as f64;

        // Per-series readout: look up the value at the anchor timestamp by
        // nearest-in-time.  Series share the same Resolution grid, so this
        // typically hits the same timestamp exactly.
        let rows: Vec<HoverRow> = s.iter().filter_map(|ser| {
            if ser.points.is_empty() { return None; }
            let nearest = ser.points.iter().min_by(|a, b| {
                let da = (a.0.timestamp() as f64 - anchor_secs).abs();
                let db = (b.0.timestamp() as f64 - anchor_secs).abs();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })?;
            Some(HoverRow {
                color: ser.color.clone(),
                label: ser.label.clone(),
                timestamp: nearest.0,
                value: nearest.1,
                point_x: g.to_x(nearest.0.timestamp()),
                point_y: g.to_y(nearest.1),
            })
        }).collect();

        if rows.is_empty() { return None; }

        let crosshair_x = g.to_x(anchor_ts.timestamp());

        // Flip tooltip side when too close to the viewport edge.  Thresholds
        // are rough estimates of tooltip dimensions — exact pixels don't
        // matter, only that the tooltip lands inside the viewport.
        let viewport = web_sys::window()
            .map(|w| (
                w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1920.0),
                w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(1080.0),
            ))
            .unwrap_or((1920.0, 1080.0));
        let flip_x = client_x + 300.0 > viewport.0;
        // For touch input, force the tooltip above the contact point so the
        // finger doesn't cover it.
        let flip_y = force_flip_y || client_y + 200.0 > viewport.1;

        Some(HoverInfo { crosshair_x, client_x, client_y, flip_x, flip_y, rows })
    };

    let on_move = move |ev: web_sys::MouseEvent| {
        let Some(target) = ev.current_target() else { return };
        let Ok(elem) = target.dyn_into::<web_sys::Element>() else { return };
        let rect = elem.get_bounding_client_rect();
        set_hover.set(compute_hover(ev.client_x() as f64, ev.client_y() as f64, rect, false));
    };
    let on_leave = move |_: web_sys::MouseEvent| set_hover.set(None);
    let on_touch = move |ev: web_sys::TouchEvent| {
        // Block page scroll while scrubbing the chart with a finger, and
        // stop the event from bubbling to the window-level "clear hover"
        // listener — otherwise our own taps would dismiss the tooltip.
        ev.prevent_default();
        ev.stop_propagation();
        let Some(touch) = ev.touches().get(0) else { return };
        let Some(target) = ev.current_target() else { return };
        let Ok(elem) = target.dyn_into::<web_sys::Element>() else { return };
        let rect = elem.get_bounding_client_rect();
        // force_flip_y=true so the tooltip floats above the finger.
        set_hover.set(compute_hover(touch.client_x() as f64, touch.client_y() as f64, rect, true));
    };

    view! {
        <div class="chart-container">
            // ── Chart SVG ──
            {move || match derived() {
                None => view! { <div class="placeholder">"Please select one or more locations to view counts"</div> }.into_any(),
                Some((paths, y_ticks, x_ticks)) => view! {
                    <svg viewBox=view_box preserveAspectRatio="none">
                        // Grid lines
                        <g class="chart-grid">
                            {y_ticks.iter().map(|(y, _)| view! {
                                <line x1=pad_l y1=*y x2=(pad_l + w) y2=*y />
                            }).collect_view()}
                        </g>

                        // Area fills (solid regardless of modality dash pattern)
                        {paths.iter().map(|(color, _, _, area_d)| view! {
                            <path d=area_d.clone()
                                  fill=color.clone() class="chart-area" />
                        }).collect_view()}

                        // Lines
                        {paths.iter().map(|(color, dash, line_d, _)| view! {
                            <path d=line_d.clone() class="chart-line"
                                  stroke=color.clone()
                                  stroke-dasharray=dash.clone() />
                        }).collect_view()}

                        // Y axis
                        <g class="chart-axis">
                            <line x1=pad_l y1=pad_t x2=pad_l y2=(pad_t + h) />
                            {y_ticks.iter().map(|(y, label)| view! {
                                <g>
                                    <line x1=(pad_l - 4.0) y1=*y x2=pad_l y2=*y />
                                    <text x=(pad_l - 8.0) y=*y
                                          text-anchor="end" dominant-baseline="middle">
                                        {label.clone()}
                                    </text>
                                </g>
                            }).collect_view()}
                        </g>

                        // X axis
                        <g class="chart-axis">
                            <line x1=pad_l y1=(pad_t + h) x2=(pad_l + w) y2=(pad_t + h) />
                            {x_ticks.iter().map(|(x, label)| view! {
                                <g>
                                    <line x1=*x y1=(pad_t + h) x2=*x y2=(pad_t + h + 4.0) />
                                    <text x=*x y=(pad_t + h + 16.0)
                                          text-anchor="middle">{label.clone()}</text>
                                </g>
                            }).collect_view()}
                        </g>

                        // Transparent hover-target rect.  Drawn after the chart
                        // contents but visually invisible — captures mouse moves
                        // for the crosshair tooltip.
                        <rect class="chart-hover-area"
                              x=pad_l y=pad_t width=w height=h
                              fill="transparent"
                              on:mousemove=on_move
                              on:mouseleave=on_leave
                              on:touchstart=on_touch.clone()
                              on:touchmove=on_touch />

                        // Crosshair + per-series dots, drawn last so they sit
                        // above everything else.  pointer-events: none in CSS
                        // so they don't intercept mousemove from the rect.
                        {move || hover.get().map(|hi| view! {
                            <g class="chart-hover">
                                <line class="chart-crosshair"
                                      x1=hi.crosshair_x y1=pad_t
                                      x2=hi.crosshair_x y2=(pad_t + h) />
                                {hi.rows.iter().map(|row| view! {
                                    <circle class="chart-hover-dot"
                                            cx=row.point_x cy=row.point_y
                                            r="4"
                                            fill=row.color.clone() />
                                }).collect_view()}
                            </g>
                        })}
                    </svg>
                }.into_any(),
            }}

            // ── Hover tooltip (HTML, position: fixed to avoid clipping) ──
            {move || hover.get().map(|hi| {
                let res = resolution.get();
                let date = hi.rows.first()
                    .map(|r| format_hover_date(r.timestamp, res))
                    .unwrap_or_default();
                // CSS transform flips the tooltip's anchor across the cursor
                // when we're too close to a viewport edge to fit on the
                // default (lower-right) side.
                let tx = if hi.flip_x { "calc(-100% - 14px)" } else { "14px" };
                let ty = if hi.flip_y { "calc(-100% - 14px)" } else { "14px" };
                view! {
                    <div class="chart-tooltip"
                         style=format!(
                             "left: {}px; top: {}px; transform: translate({}, {});",
                             hi.client_x, hi.client_y, tx, ty,
                         )>
                        <div class="chart-tooltip-date">{date}</div>
                        {hi.rows.iter().map(|row| {
                            let value = fmt_count(row.value);
                            view! {
                                <div class="chart-tooltip-row">
                                    <span class="chart-tooltip-swatch"
                                          style=format!("background: {};", row.color)></span>
                                    <span class="chart-tooltip-label">{row.label.clone()}</span>
                                    <span class="chart-tooltip-value">{value}</span>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                }
            })}

            // ── Legend ──
            {move || {
                let s = series.get();
                if s.is_empty() { return view! { <div></div> }.into_any(); }
                view! {
                    <div class="chart-legend">
                        {s.into_iter().map(|ser| {
                            let stats = series_stats(&ser.points);
                            view! {
                                <div class="chart-legend-item">
                                    <svg width="24" height="10">
                                        <line x1="1" y1="5" x2="23" y2="5"
                                              stroke=ser.color.clone()
                                              stroke-width="2"
                                              stroke-dasharray=ser.dash.clone() />
                                    </svg>
                                    <span>{ser.label.clone()}</span>
                                    <span class="legend-stats">{stats}</span>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}
        </div>
    }
}
