use chrono::{DateTime, Utc};
use leptos::prelude::*;

#[derive(Clone, PartialEq)]
pub struct Series {
    pub label: String,
    pub color: String,
    pub points: Vec<(DateTime<Utc>, f64)>,
}

#[component]
pub fn Chart(series: ReadSignal<Vec<Series>>) -> impl IntoView {
    let view_box = "0 0 900 400";
    let pad_l = 60.0_f64;
    let pad_r = 20.0_f64;
    let pad_t = 20.0_f64;
    let pad_b = 50.0_f64;
    let w = 900.0_f64 - pad_l - pad_r;
    let h = 400.0_f64 - pad_t - pad_b;

    let derived = move || {
        let s = series.get();
        if s.is_empty() || s.iter().all(|ser| ser.points.is_empty()) {
            return None;
        }

        let all_x: Vec<i64> = s.iter()
            .flat_map(|ser| ser.points.iter().map(|(dt, _)| dt.timestamp()))
            .collect();
        let all_y: Vec<f64> = s.iter()
            .flat_map(|ser| ser.points.iter().map(|(_, v)| *v))
            .collect();

        let x_min = *all_x.iter().min().unwrap() as f64;
        let x_max = *all_x.iter().max().unwrap() as f64;
        let y_min = 0.0_f64;
        let y_max = all_y.iter().cloned().fold(0.0_f64, f64::max) * 1.1;

        let x_span = (x_max - x_min).max(1.0);
        let y_span = (y_max - y_min).max(1.0);

        let to_x = |ts: i64| pad_l + (ts as f64 - x_min) / x_span * w;
        let to_y = |v: f64| pad_t + h - (v - y_min) / y_span * h;

        // Build polyline path strings
        let paths: Vec<(String, String, String)> = s.iter().map(|ser| {
            if ser.points.is_empty() {
                return (ser.color.clone(), String::new(), String::new());
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
            (ser.color.clone(), line_d, area_d)
        }).collect();

        // Y-axis ticks
        let tick_count = 5;
        let y_ticks: Vec<(f64, String)> = (0..=tick_count).map(|i| {
            let v = y_min + (y_max - y_min) * i as f64 / tick_count as f64;
            (to_y(v), format!("{:.0}", v))
        }).collect();

        // X-axis ticks (up to 8)
        let x_tick_n = 7.min(s[0].points.len());
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
            {move || match derived() {
                None => view! { <div class="placeholder">"Select a data source to begin"</div> }.into_any(),
                Some((paths, y_ticks, x_ticks)) => view! {
                    <svg viewBox=view_box preserveAspectRatio="none">
                        // Grid lines
                        <g class="chart-grid">
                            {y_ticks.iter().map(|(y, _)| view! {
                                <line x1=pad_l y1=*y x2=(pad_l + w) y2=*y />
                            }).collect_view()}
                        </g>

                        // Area fills
                        {paths.iter().map(|(color, _, area_d)| view! {
                            <path d=area_d.clone()
                                  fill=color.clone() class="chart-area" />
                        }).collect_view()}

                        // Lines
                        {paths.iter().map(|(color, line_d, _)| view! {
                            <path d=line_d.clone() class="chart-line"
                                  stroke=color.clone() />
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
        </div>
    }
}
