use std::collections::BTreeMap;
use std::io::Cursor;

use chrono::{DateTime, Datelike, LocalResult, TimeZone, Utc};
use chrono_tz::America::Montreal as MontrealTz;

use crate::data::sources::{MONTREAL_CYCLISTES_URL, MONTREAL_LOCATION_FILTER, SOURCE_COLORS};
use crate::data::types::{CountRecord, DataSource, LatLon, LoaderType, Modality, Resolution};

// ── CSV helpers ──────────────────────────────────────────────────────────────

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

fn find_col(headers: &[String], name: &str) -> Option<usize> {
    headers.iter().position(|h| h.trim().eq_ignore_ascii_case(name))
}

fn parse_montreal_ts(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    let normalised = if s.len() == 22 { format!("{}:00", s) } else { s.to_string() };
    DateTime::parse_from_str(&normalised, "%Y-%m-%d %H:%M:%S%:z")
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

// ── Montreal Cyclistes CSV ───────────────────────────────────────────────────

struct InstanceAcc {
    lat: f64,
    lon: f64,
    rue1: String,
    rue2: String,
    direction: String,
    color_idx: usize,
    /// Hourly records (agg_code = "h") keyed by exact UTC timestamp.
    /// Preferred over daily when any exist.
    hourly: BTreeMap<i64, f64>,
    /// Daily records (agg_code = "d") keyed by UTC midnight timestamp.
    /// Used only as fallback when no hourly records exist for this instance.
    daily: BTreeMap<i64, f64>,
}

/// Parse the City of Montreal cyclistes CSV.
/// Accepts agg_code "h" (hourly) and "d" (daily).
/// Hourly records are preferred; daily are kept only as a fallback for
/// instances that have no hourly rows, preventing double-counting.
/// One `DataSource` per unique `(instance, direction)` pair.
pub fn parse_montreal_cyclistes_csv(text: &str) -> (Vec<DataSource>, Vec<CountRecord>) {
    let mut lines = text.lines();
    let Some(header_line) = lines.next() else { return (vec![], vec![]) };
    let headers: Vec<String> = split_csv_line(header_line);

    let Some(agg_col)      = find_col(&headers, "agg_code")  else { return (vec![], vec![]) };
    let Some(instance_col) = find_col(&headers, "instance")  else { return (vec![], vec![]) };
    let Some(lat_col)      = find_col(&headers, "latitude")  else { return (vec![], vec![]) };
    let Some(lon_col)      = find_col(&headers, "longitude") else { return (vec![], vec![]) };
    let Some(periode_col)  = find_col(&headers, "periode")   else { return (vec![], vec![]) };
    let Some(volume_col)   = find_col(&headers, "volume")    else { return (vec![], vec![]) };
    let rue1_col      = find_col(&headers, "rue_1");
    let rue2_col      = find_col(&headers, "rue_2");
    let direction_col = find_col(&headers, "direction");

    let mut instances: std::collections::HashMap<(String, String), InstanceAcc>
        = std::collections::HashMap::new();
    let mut color_counter = 0usize;

    for line in lines {
        if line.trim().is_empty() { continue; }
        let fields = split_csv_line(line);
        let get = |col: usize| fields.get(col).map(|s| s.trim().to_string()).unwrap_or_default();

        let agg = get(agg_col);
        // Accept hourly ("h") and daily ("d") only; skip 15-min, monthly, annual
        if agg != "h" && agg != "d" { continue; }

        let instance  = get(instance_col);
        let direction = direction_col.map(|c| get(c)).unwrap_or_default();
        if instance.is_empty() { continue; }

        let lat: f64 = match get(lat_col).parse()       { Ok(v) => v, Err(_) => continue };
        let lon: f64 = match get(lon_col).parse()       { Ok(v) => v, Err(_) => continue };
        let volume: f64 = match get(volume_col).parse() { Ok(v) => v, Err(_) => continue };
        let ts = match parse_montreal_ts(&get(periode_col)) { Some(t) => t, None => continue };

        let rue1 = rue1_col.map(|c| get(c)).unwrap_or_default();
        let rue2 = rue2_col.map(|c| get(c)).unwrap_or_default();

        if let Some(filters) = MONTREAL_LOCATION_FILTER {
            let r1 = rue1.to_lowercase();
            let r2 = rue2.to_lowercase();
            let ok = filters.iter().any(|(f1, f2)| {
                r1.contains(&f1.to_lowercase()) && r2.contains(&f2.to_lowercase())
            });
            if !ok { continue; }
        }

        let key = (instance.clone(), direction.clone());
        let acc = instances.entry(key).or_insert_with(|| {
            let idx = color_counter % SOURCE_COLORS.len();
            color_counter += 1;
            InstanceAcc { lat, lon, rue1: rue1.clone(), rue2: rue2.clone(),
                          direction: direction.clone(), color_idx: idx,
                          hourly: BTreeMap::new(), daily: BTreeMap::new() }
        });

        if agg == "h" {
            // Store at exact hourly timestamp
            *acc.hourly.entry(ts.timestamp()).or_insert(0.0) += volume;
        } else {
            // Store daily at UTC midnight
            let day_key = ts.date_naive().and_hms_opt(0,0,0).unwrap().and_utc().timestamp();
            *acc.daily.entry(day_key).or_insert(0.0) += volume;
        }
    }

    let mut sources = Vec::with_capacity(instances.len());
    let mut records = Vec::new();
    let mut list: Vec<_> = instances.into_iter().collect();
    list.sort_by(|a, b| a.0.cmp(&b.0));

    for ((instance_id, direction), acc) in list {
        // Prefer hourly records; fall back to daily if no hourly exist
        let chosen = if !acc.hourly.is_empty() { &acc.hourly } else { &acc.daily };
        if chosen.is_empty() { continue; }

        let dir_slug = direction.to_lowercase().replace(' ', "-");
        let source_id = if dir_slug.is_empty() {
            format!("mtl-{}", instance_id)
        } else {
            format!("mtl-{}-{}", instance_id, dir_slug)
        };
        let name = match (acc.rue2.is_empty(), direction.is_empty()) {
            (false, false) => format!("Mtl: {} @ {} ({})", acc.rue1, acc.rue2, direction),
            (false, true)  => format!("Mtl: {} @ {}",      acc.rue1, acc.rue2),
            (true,  false) => format!("Mtl: {} ({})",       acc.rue1, direction),
            (true,  true)  => format!("Mtl: {}",             acc.rue1),
        };

        let earliest = DateTime::from_timestamp(*chosen.keys().next().unwrap(), 0).unwrap();
        let latest   = DateTime::from_timestamp(*chosen.keys().next_back().unwrap(), 0).unwrap();

        sources.push(DataSource {
            id: source_id.clone(), name,
            location: LatLon { lat: acc.lat, lon: acc.lon },
            modalities: vec![Modality::Bikes],
            earliest, latest,
            color: SOURCE_COLORS[acc.color_idx].to_string(),
            loader_type: LoaderType::Discovered,
        });
        for (ts_unix, total) in chosen {
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

pub async fn fetch_montreal_cyclistes() -> Result<(Vec<DataSource>, Vec<CountRecord>), String> {
    let resp = gloo_net::http::Request::get(MONTREAL_CYCLISTES_URL)
        .send().await
        .map_err(|e| format!("Network error: {:?}", e))?;
    if !resp.ok() { return Err(format!("HTTP {}", resp.status())); }
    let text = resp.text().await.map_err(|e| format!("Body error: {:?}", e))?;
    Ok(parse_montreal_cyclistes_csv(&text))
}

// ── Telraam S2 Excel ─────────────────────────────────────────────────────────

/// Parse a Telraam S2 Excel export (`.xlsx`).
///
/// The format has one sheet (`Worksheet instances`), one row per hour, with
/// columns detected by name (case-insensitive).  Counts from both directions
/// are taken from the `* Total` columns.  Timestamps are in Montreal local
/// time and are converted to UTC via DST-aware lookup.
pub fn parse_telraam_excel(source_id: &str, bytes: &[u8]) -> Vec<CountRecord> {
    use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};

    let cursor = Cursor::new(bytes.to_vec());
    let mut wb: Xlsx<_> = match open_workbook_from_rs(cursor) {
        Ok(w) => w,
        Err(e) => {
            web_sys::console::error_1(&format!("Telraam Excel open failed: {:?}", e).into());
            return vec![];
        }
    };

    let sheet_names = wb.sheet_names().to_vec();
    let Some(sheet) = sheet_names.first().cloned() else { return vec![] };
    let range = match wb.worksheet_range(&sheet) {
        Ok(r) => r,
        Err(e) => {
            web_sys::console::error_1(&format!("Telraam sheet read failed: {:?}", e).into());
            return vec![];
        }
    };

    let mut rows = range.rows();
    let Some(header) = rows.next() else { return vec![] };

    // Helpers for reading calamine cell values via pattern matching
    fn cell_str(c: &Data) -> Option<&str> {
        match c {
            Data::String(s)      => Some(s.as_str()),
            Data::DateTimeIso(s) => Some(s.as_str()),
            Data::DurationIso(s) => Some(s.as_str()),
            _ => None,
        }
    }
    let cell_f64 = |c: &Data| -> f64 {
        match c {
            Data::Float(f) => *f,
            Data::Int(i)   => *i as f64,
            Data::Bool(b)  => if *b { 1.0 } else { 0.0 },
            _ => 0.0,
        }
    };

    // Detect columns by case-insensitive name
    let find = |name: &str| -> Option<usize> {
        header.iter().position(|c| {
            cell_str(c).map_or(false, |s| s.trim().eq_ignore_ascii_case(name))
        })
    };

    let Some(dt_col)   = find("date and time (local)") else { return vec![] };
    let Some(bike_col) = find("bike total")             else { return vec![] };
    let Some(ped_col)  = find("pedestrian total")       else { return vec![] };
    let Some(car_col)  = find("car total")              else { return vec![] };
    let large_col      = find("large vehicle total");

    let as_f64 = |cell: Option<&Data>| -> f64 {
        cell.map(cell_f64).unwrap_or(0.0)
    };

    let mut records = Vec::new();

    for row in rows {
        let Some(dt_str) = row.get(dt_col).and_then(|c| cell_str(c)) else { continue };
        let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%d %H:%M")
            else { continue };

        // DST-aware conversion from Montreal local to UTC
        let ts = match MontrealTz.from_local_datetime(&ndt) {
            LocalResult::Single(dt)        => dt.with_timezone(&Utc),
            LocalResult::Ambiguous(dt, _)  => dt.with_timezone(&Utc), // fall-back hour: use first
            LocalResult::None              => continue,                // spring-forward gap
        };

        let bike  = as_f64(row.get(bike_col));
        let ped   = as_f64(row.get(ped_col));
        let car   = as_f64(row.get(car_col));
        let truck = large_col.map(|c| as_f64(row.get(c))).unwrap_or(0.0);

        for (modality, count) in [
            (Modality::Bikes,       bike),
            (Modality::Pedestrians, ped),
            (Modality::Cars,        car),
            (Modality::Trucks,      truck),
        ] {
            records.push(CountRecord {
                timestamp: ts,
                modality,
                count,
                source_id: source_id.to_string(),
            });
        }
    }

    records
}

/// Fetch a Telraam Excel file from a URL and parse it.
pub async fn fetch_telraam_excel(source_id: &str, url: &str) -> Result<Vec<CountRecord>, String> {
    let resp = gloo_net::http::Request::get(url)
        .send().await
        .map_err(|e| format!("Fetch error: {:?}", e))?;
    if !resp.ok() { return Err(format!("HTTP {}", resp.status())); }
    let bytes = resp.binary().await.map_err(|e| format!("Binary error: {:?}", e))?;
    Ok(parse_telraam_excel(source_id, &bytes))
}

// ── Aggregation ──────────────────────────────────────────────────────────────

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
