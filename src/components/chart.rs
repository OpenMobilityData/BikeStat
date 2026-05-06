use chrono::{DateTime, Utc};
use leptos::prelude::*;

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

#[component]
pub fn Chart(
    series: ReadSignal<Vec<Series>>,
    /// When Some, fixes the X-axis to the given (start, end) range instead of
    /// auto-deriving it from the points. Used by Year-on-Year so the axis
    /// always shows a full 12-month span even with partial data.
    x_range: ReadSignal<Option<(DateTime<Utc>, DateTime<Utc>)>>,
) -> impl IntoView {
    let view_box = "0 0 900 400";
    let pad_l = 60.0_f64;
    let pad_r = 20.0_f64;
    let pad_t = 20.0_f64;
    let pad_b = 50.0_f64;
    let w = 900.0_f64 - pad_l - pad_r;
    let h = 400.0_f64 - pad_t - pad_b;

    let derived = move || {
        let s = series.get();
        let xr = x_range.get();
        if (s.is_empty() || s.iter().all(|ser| ser.points.is_empty())) && xr.is_none() {
            return None;
        }

        let all_y: Vec<f64> = s.iter()
            .flat_map(|ser| ser.points.iter().map(|(_, v)| *v))
            .collect();

        let (x_min, x_max) = match xr {
            Some((lo, hi)) => (lo.timestamp() as f64, hi.timestamp() as f64),
            None => {
                let all_x: Vec<i64> = s.iter()
                    .flat_map(|ser| ser.points.iter().map(|(dt, _)| dt.timestamp()))
                    .collect();
                (*all_x.iter().min().unwrap() as f64,
                 *all_x.iter().max().unwrap() as f64)
            }
        };
        let y_min = 0.0_f64;
        let y_max = all_y.iter().cloned().fold(0.0_f64, f64::max) * 1.1;

        let x_span = (x_max - x_min).max(1.0);
        let y_span = (y_max - y_min).max(1.0);

        let to_x = |ts: i64| pad_l + (ts as f64 - x_min) / x_span * w;
        let to_y = |v: f64| pad_t + h - (v - y_min) / y_span * h;

        // Build polyline path strings
        let paths: Vec<(String, String, String, String)> = s.iter().map(|ser| {
            if ser.points.is_empty() {
                return (ser.color.clone(), ser.dash.clone(), String::new(), String::new());
            }
            let pts: Vec<String> = ser.points.iter()
                .map(|(dt, v)| format!("{:.1},{:.1}", to_x(dt.timestamp()), to_y(*v)))
                .collect();
            let line_d = format!("M {}", pts.join(" L "));

            // area path: close down to y-axis baseline
            let first_x = to_x(ser.points[0].0.timestamp());
            let last_x  = to_x(ser.points.last().unwrap().0.timestamp());
            let base_y  = to_y(y_min);
            let area_d  = format!("M {},{} L {} L {},{} Z",
                first_x, base_y, pts.join(" L "), last_x, base_y);
            (ser.color.clone(), ser.dash.clone(), line_d, area_d)
        }).collect();

        // Y-axis ticks
        let tick_count = 5;
        let y_ticks: Vec<(f64, String)> = (0..=tick_count).map(|i| {
            let v = y_min + (y_max - y_min) * i as f64 / tick_count as f64;
            (to_y(v), format!("{:.0}", v))
        }).collect();

        // X-axis ticks (up to 8)
        let max_points = s.iter().map(|ser| ser.points.len()).max().unwrap_or(0);
        let x_tick_n = 7.min(max_points.max(if xr.is_some() { 7 } else { 0 }));
        let x_ticks: Vec<(f64, String)> = if x_tick_n == 0 { vec![] } else {
            (0..=x_tick_n).map(|i| {
                let ts = (x_min + x_span * i as f64 / x_tick_n as f64) as i64;
                let x = pad_l + (ts as f64 - x_min) / x_span * w;
                let label = DateTime::from_timestamp(ts, 0)
                    .map(|dt: DateTime<Utc>| dt.format("%b %d").to_string())
                    .unwrap_or_default();
                (x, label)
            }).collect()
        };

        Some((paths, y_ticks, x_ticks))
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
                    </svg>
                }.into_any(),
            }}

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
