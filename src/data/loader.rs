use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Utc};

use crate::data::sources::{MONTREAL_CYCLISTES_URL, MONTREAL_LOCATION_FILTER, SOURCE_COLORS};
use crate::data::types::{CountRecord, DataSource, LatLon, LoaderType, Modality, Resolution};

// ── CSV helpers ──────────────────────────────────────────────────────────────

/// Split one CSV line into fields, respecting double-quote enclosures.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    for c in line.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => { fields.push(std::mem::take(&mut field)); }
            _ => field.push(c),
        }
    }
    fields.push(field);
    fields
}

/// Find a column index by case-insensitive name match.
fn find_col(headers: &[String], name: &str) -> Option<usize> {
    headers.iter().position(|h| h.trim().eq_ignore_ascii_case(name))
}

/// Parse Montreal's `periode` timestamp: `"2025-11-04 00:00:00-05"`.
/// Normalises the short UTC offset (`-05`) to `±HH:MM` for chrono.
fn parse_montreal_ts(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    // "YYYY-MM-DD HH:MM:SS±HH"  →  "YYYY-MM-DD HH:MM:SS±HH:MM"
    let normalised = if s.len() == 22 { format!("{}:00", s) } else { s.to_string() };
    DateTime::parse_from_str(&normalised, "%Y-%m-%d %H:%M:%S%:z")
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

// ── Montreal Cyclistes CSV ───────────────────────────────────────────────────

/// Accumulator for one (instance, direction) pair while scanning the CSV.
struct InstanceAcc {
    lat: f64,
    lon: f64,
    rue1: String,
    rue2: String,
    direction: String,
    color_idx: usize,
    /// Daily volumes keyed by Unix timestamp of UTC midnight for that day.
    volumes: BTreeMap<i64, f64>,
}

/// Parse the City of Montreal cyclistes CSV.
///
/// Only rows with `agg_code = "d"` (daily) are used, preventing double-counting
/// when both hourly and daily records exist for the same counter.
///
/// One `DataSource` is created per unique `(instance, direction)` pair so the
/// user can select individual directions independently.  If
/// `MONTREAL_LOCATION_FILTER` is set, only matching intersections are included.
pub fn parse_montreal_cyclistes_csv(text: &str) -> (Vec<DataSource>, Vec<CountRecord>) {
    let mut lines = text.lines();
    let Some(header_line) = lines.next() else { return (vec![], vec![]) };

    let headers: Vec<String> = split_csv_line(header_line);

    // Required columns
    let Some(agg_col)      = find_col(&headers, "agg_code")  else { return (vec![], vec![]) };
    let Some(instance_col) = find_col(&headers, "instance")  else { return (vec![], vec![]) };
    let Some(lat_col)      = find_col(&headers, "latitude")  else { return (vec![], vec![]) };
    let Some(lon_col)      = find_col(&headers, "longitude") else { return (vec![], vec![]) };
    let Some(periode_col)  = find_col(&headers, "periode")   else { return (vec![], vec![]) };
    let Some(volume_col)   = find_col(&headers, "volume")    else { return (vec![], vec![]) };

    let rue1_col      = find_col(&headers, "rue_1");
    let rue2_col      = find_col(&headers, "rue_2");
    let direction_col = find_col(&headers, "direction");

    // (instance_id, direction) → accumulator
    let mut instances: std::collections::HashMap<(String, String), InstanceAcc>
        = std::collections::HashMap::new();
    let mut color_counter = 0usize;

    for line in lines {
        if line.trim().is_empty() { continue; }
        let fields = split_csv_line(line);
        let get = |col: usize| fields.get(col).map(|s| s.trim().to_string()).unwrap_or_default();

        // Only process daily-aggregated records
        if get(agg_col) != "d" { continue; }

        let instance  = get(instance_col);
        let direction = direction_col.map(|c| get(c)).unwrap_or_default();
        if instance.is_empty() { continue; }

        let lat: f64 = match get(lat_col).parse()    { Ok(v) => v, Err(_) => continue };
        let lon: f64 = match get(lon_col).parse()    { Ok(v) => v, Err(_) => continue };
        let volume: f64 = match get(volume_col).parse() { Ok(v) => v, Err(_) => continue };
        let ts = match parse_montreal_ts(&get(periode_col)) { Some(t) => t, None => continue };

        let rue1 = rue1_col.map(|c| get(c)).unwrap_or_default();
        let rue2 = rue2_col.map(|c| get(c)).unwrap_or_default();

        // Apply location whitelist
        if let Some(filters) = MONTREAL_LOCATION_FILTER {
            let r1 = rue1.to_lowercase();
            let r2 = rue2.to_lowercase();
            let passes = filters.iter().any(|(f1, f2)| {
                r1.contains(&f1.to_lowercase()) && r2.contains(&f2.to_lowercase())
            });
            if !passes { continue; }
        }

        // Bucket to the UTC day boundary
        let day_key = ts.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();

        let key = (instance.clone(), direction.clone());
        let acc = instances.entry(key).or_insert_with(|| {
            let idx = color_counter % SOURCE_COLORS.len();
            color_counter += 1;
            InstanceAcc { lat, lon, rue1: rue1.clone(), rue2: rue2.clone(),
                          direction: direction.clone(), color_idx: idx,
                          volumes: BTreeMap::new() }
        });

        *acc.volumes.entry(day_key).or_insert(0.0) += volume;
    }

    let mut sources = Vec::with_capacity(instances.len());
    let mut records = Vec::new();

    // Sort for stable ordering
    let mut list: Vec<_> = instances.into_iter().collect();
    list.sort_by(|a, b| a.0.cmp(&b.0));

    for ((instance_id, direction), acc) in list {
        if acc.volumes.is_empty() { continue; }

        // Unique source ID encodes both instance and direction
        let dir_slug = direction.to_lowercase().replace(' ', "-");
        let source_id = if dir_slug.is_empty() {
            format!("mtl-{}", instance_id)
        } else {
            format!("mtl-{}-{}", instance_id, dir_slug)
        };

        // Human-readable name includes direction where it adds information
        let name = match (acc.rue2.is_empty(), direction.is_empty()) {
            (false, false) => format!("Mtl: {} @ {} ({})", acc.rue1, acc.rue2, direction),
            (false, true)  => format!("Mtl: {} @ {}",      acc.rue1, acc.rue2),
            (true,  false) => format!("Mtl: {} ({})",       acc.rue1, direction),
            (true,  true)  => format!("Mtl: {}",             acc.rue1),
        };

        let earliest = DateTime::from_timestamp(*acc.volumes.keys().next().unwrap(), 0).unwrap();
        let latest   = DateTime::from_timestamp(*acc.volumes.keys().next_back().unwrap(), 0).unwrap();

        sources.push(DataSource {
            id: source_id.clone(),
            name,
            location: LatLon { lat: acc.lat, lon: acc.lon },
            modalities: vec![Modality::Bikes],
            earliest,
            latest,
            color: SOURCE_COLORS[acc.color_idx].to_string(),
            loader_type: LoaderType::Discovered,
        });

        for (ts_unix, total) in &acc.volumes {
            records.push(CountRecord {
                timestamp: DateTime::from_timestamp(*ts_unix, 0).unwrap(),
                modality:  Modality::Bikes,
                count:     *total,
                source_id: source_id.clone(),
            });
        }
    }

    (sources, records)
}

/// Fetch the Montreal cyclistes CSV from the open data portal and parse it.
pub async fn fetch_montreal_cyclistes() -> Result<(Vec<DataSource>, Vec<CountRecord>), String> {
    let resp = gloo_net::http::Request::get(MONTREAL_CYCLISTES_URL)
        .send()
        .await
        .map_err(|e| format!("Network error: {:?}", e))?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let text = resp.text().await.map_err(|e| format!("Body error: {:?}", e))?;
    Ok(parse_montreal_cyclistes_csv(&text))
}

// ── Telraam S2 Excel ─────────────────────────────────────────────────────────
//
// Not yet implemented — awaiting example files.
//
// Expected column layout (detected case-insensitively):
//   date / datetime / timestamp   — date or date+hour
//   hour                          — when date-only column is used
//   pedestrian / ped              — pedestrian count
//   bike / cyclist                — bicycle count
//   car / motorized               — car count
//   heavy / truck                 — heavy-vehicle count
//
// Once an example file is available, implement:
//
//   pub fn parse_telraam_excel(source_id: &str, bytes: &[u8]) -> Vec<CountRecord>
//
//   pub async fn fetch_telraam_excel(source_id: &str, url: &str) -> Result<Vec<CountRecord>, String>

// ── Aggregation ──────────────────────────────────────────────────────────────

/// Group records by time bucket and sum counts.
/// If `source_id` is `Some`, only that source is included.
pub fn aggregate(
    records: &[CountRecord],
    modality: Modality,
    resolution: Resolution,
    source_id: Option<&str>,
) -> Vec<(DateTime<Utc>, f64)> {
    let mut buckets: BTreeMap<i64, f64> = BTreeMap::new();

    for rec in records {
        if rec.modality != modality { continue; }
        if source_id.map_or(false, |id| rec.source_id != id) { continue; }
        let key = bucket_key(rec.timestamp, resolution);
        *buckets.entry(key).or_insert(0.0) += rec.count;
    }

    buckets.into_iter()
        .filter_map(|(k, v)| DateTime::from_timestamp(k, 0).map(|dt| (dt, v)))
        .collect()
}

fn bucket_key(ts: DateTime<Utc>, res: Resolution) -> i64 {
    use chrono::Timelike;
    match res {
        Resolution::Hour => ts
            .with_minute(0).unwrap().with_second(0).unwrap().with_nanosecond(0).unwrap()
            .timestamp(),
        Resolution::Day => ts.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp(),
        Resolution::Week => {
            let dow = ts.weekday().num_days_from_monday();
            (ts.date_naive() - chrono::Duration::days(dow as i64))
                .and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
        }
        Resolution::Month => ts
            .date_naive().with_day(1).unwrap()
            .and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp(),
    }
}
