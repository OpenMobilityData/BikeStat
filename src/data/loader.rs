use std::collections::BTreeMap;
use std::io::Cursor;

use chrono::{DateTime, Datelike, LocalResult, TimeZone, Utc};
use chrono_tz::America::Montreal as MontrealTz;

use crate::data::sources::{telraam_annotation, MONTREAL_CYCLISTES_URL, MONTREAL_LOCATION_FILTER, SOURCE_COLORS};
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

fn make_total_id(rue1: &str, rue2: &str) -> String {
    let slug = |s: &str| s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>();
    format!("mtl-{}-{}-total", slug(rue1), slug(rue2))
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
    // Order intersections by their position in MONTREAL_LOCATION_FILTER so the
    // sidebar follows that list.  Within an intersection, fall back to
    // (instance_id, direction) for a stable order.  Unfiltered rows (only
    // reachable when the filter is `None`) sort after all matched ones.
    let filter_idx = |rue1: &str, rue2: &str| -> usize {
        let Some(filters) = MONTREAL_LOCATION_FILTER else { return usize::MAX };
        let r1 = rue1.to_lowercase();
        let r2 = rue2.to_lowercase();
        filters.iter().position(|(f1, f2)| {
            r1.contains(&f1.to_lowercase()) && r2.contains(&f2.to_lowercase())
        }).unwrap_or(usize::MAX)
    };
    list.sort_by(|a, b| {
        let ai = filter_idx(&a.1.rue1, &a.1.rue2);
        let bi = filter_idx(&b.1.rue1, &b.1.rue2);
        ai.cmp(&bi).then_with(|| a.0.cmp(&b.0))
    });

    // Track per-source metadata needed to build intersection totals afterwards.
    // Each entry: (source_id, rue1, rue2, lat, lon)
    let mut src_meta: Vec<(String, String, String, f64, f64)> = Vec::new();

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
            (false, false) => format!("VdM: {} @ {} ({})", acc.rue1, acc.rue2, direction),
            (false, true)  => format!("VdM: {} @ {}",      acc.rue1, acc.rue2),
            (true,  false) => format!("VdM: {} ({})",       acc.rue1, direction),
            (true,  true)  => format!("VdM: {}",             acc.rue1),
        };

        let earliest = DateTime::from_timestamp(*chosen.keys().next().unwrap(), 0).unwrap();
        let latest   = DateTime::from_timestamp(*chosen.keys().next_back().unwrap(), 0).unwrap();

        src_meta.push((source_id.clone(), acc.rue1.clone(), acc.rue2.clone(), acc.lat, acc.lon));
        sources.push(DataSource {
            id: source_id.clone(), name,
            location: LatLon { lat: acc.lat, lon: acc.lon },
            modalities: vec![Modality::Bikes],
            earliest, latest,
            color: SOURCE_COLORS[acc.color_idx].to_string(),
            loader_type: LoaderType::Discovered,
            group: Some(make_total_id(&acc.rue1, &acc.rue2)),
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

    // ── Synthetic "Total" sources for multi-direction intersections ──────────
    // Group per-direction sources by their (rue1, rue2) intersection.
    let mut by_intersection: std::collections::HashMap<
        (String, String), Vec<(String, f64, f64)> // (source_id, lat, lon)
    > = std::collections::HashMap::new();
    for (src_id, rue1, rue2, lat, lon) in &src_meta {
        by_intersection
            .entry((rue1.clone(), rue2.clone()))
            .or_default()
            .push((src_id.clone(), *lat, *lon));
    }

    let mut intersection_list: Vec<_> = by_intersection.into_iter().collect();
    intersection_list.sort_by(|a, b| a.0.cmp(&b.0));

    for ((rue1, rue2), members) in intersection_list {
        if members.len() < 2 { continue; }

        let total_id = make_total_id(&rue1, &rue2);
        let name = if rue2.is_empty() {
            format!("VdM: {} (Total)", rue1)
        } else {
            format!("VdM: {} @ {} (Total)", rue1, rue2)
        };
        let (lat, lon) = (members[0].1, members[0].2);
        let color = SOURCE_COLORS[color_counter % SOURCE_COLORS.len()].to_string();
        color_counter += 1;

        // Sum records from all member sources, bucketed by exact timestamp.
        let mut total_buckets: BTreeMap<i64, f64> = BTreeMap::new();
        for rec in &records {
            if members.iter().any(|(id, _, _)| id == &rec.source_id) {
                *total_buckets.entry(rec.timestamp.timestamp()).or_insert(0.0) += rec.count;
            }
        }
        if total_buckets.is_empty() { continue; }

        let earliest = DateTime::from_timestamp(*total_buckets.keys().next().unwrap(), 0).unwrap();
        let latest   = DateTime::from_timestamp(*total_buckets.keys().next_back().unwrap(), 0).unwrap();

        // Insert immediately after the last directional source for this
        // intersection so the total stays grouped with its siblings.
        let insert_pos = members.iter()
            .filter_map(|(id, _, _)| sources.iter().rposition(|s| &s.id == id))
            .max()
            .map(|i| i + 1)
            .unwrap_or(sources.len());
        sources.insert(insert_pos, DataSource {
            id: total_id.clone(), name,
            location: LatLon { lat, lon },
            modalities: vec![Modality::Bikes],
            earliest, latest,
            color,
            loader_type: LoaderType::Discovered,
            group: Some(total_id.clone()),
        });
        for (ts_unix, total) in total_buckets {
            records.push(CountRecord {
                timestamp: DateTime::from_timestamp(ts_unix, 0).unwrap(),
                modality:  Modality::Bikes,
                count:     total,
                source_id: total_id.clone(),
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

    // Per-direction columns (present in standard Telraam S2 exports).
    // Column name format: "{Modality} (A > B)" / "{Modality} (B > A)".
    let bike_ab  = find("bike (a > b)");
    let bike_ba  = find("bike (b > a)");
    let ped_ab   = find("pedestrian (a > b)");
    let ped_ba   = find("pedestrian (b > a)");
    let car_ab   = find("car (a > b)");
    let car_ba   = find("car (b > a)");
    let heavy_ab = find("heavy (a > b)");
    let heavy_ba = find("heavy (b > a)");

    // Only emit directional records when an annotation is registered for this
    // segment (which means the corresponding DataSources exist in the catalogue).
    let emit_dirs = telraam_annotation(source_id).is_some();
    let atob_id   = format!("{}-atob", source_id);
    let btoa_id   = format!("{}-btoa", source_id);

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

        // Total records (both directions combined)
        for (modality, count) in [
            (Modality::Bikes,       bike),
            (Modality::Pedestrians, ped),
            (Modality::Cars,        car),
            (Modality::Trucks,      truck),
        ] {
            records.push(CountRecord { timestamp: ts, modality, count,
                                       source_id: source_id.to_string() });
        }

        // Per-direction records
        if emit_dirs {
            for (sid, cols) in [
                (atob_id.as_str(), [bike_ab, ped_ab, car_ab, heavy_ab]),
                (btoa_id.as_str(), [bike_ba, ped_ba, car_ba, heavy_ba]),
            ] {
                for (modality, col_opt) in [
                    (Modality::Bikes,       cols[0]),
                    (Modality::Pedestrians, cols[1]),
                    (Modality::Cars,        cols[2]),
                    (Modality::Trucks,      cols[3]),
                ] {
                    if let Some(col) = col_opt {
                        records.push(CountRecord {
                            timestamp: ts,
                            modality,
                            count: as_f64(row.get(col)),
                            source_id: sid.to_string(),
                        });
                    }
                }
            }
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

// ── Telraam API JSON ─────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct TelraamApiResponse {
    report: Vec<TelraamApiHour>,
}

/// One hourly bucket from `POST /v1/reports/traffic`.  Counts are
/// uptime-corrected floats (Telraam scales raw counts by `1/uptime`),
/// `*_lft` / `*_rgt` are the directional splits.
#[derive(serde::Deserialize)]
struct TelraamApiHour {
    /// "YYYY-MM-DD HH:MM:SSZ" in UTC (note: space, not T, before the time).
    date: String,
    #[serde(default)] bike: Option<f64>,
    #[serde(default)] pedestrian: Option<f64>,
    #[serde(default)] car: Option<f64>,
    #[serde(default)] heavy: Option<f64>,
    #[serde(default)] bike_lft: Option<f64>,
    #[serde(default)] bike_rgt: Option<f64>,
    #[serde(default)] pedestrian_lft: Option<f64>,
    #[serde(default)] pedestrian_rgt: Option<f64>,
    #[serde(default)] car_lft: Option<f64>,
    #[serde(default)] car_rgt: Option<f64>,
    #[serde(default)] heavy_lft: Option<f64>,
    #[serde(default)] heavy_rgt: Option<f64>,
}

/// Parse a Telraam `/v1/reports/traffic` response body and emit one
/// `CountRecord` per modality per hour, plus per-direction records under
/// `<source_id>-atob` / `<source_id>-btoa` when an annotation is registered.
///
/// Direction mapping: Telraam's `*_lft` → A→B (atob), `*_rgt` → B→A (btoa).
/// If a segment's directional labels look swapped after first deploy, flip
/// these two assignments.
pub fn parse_telraam_api(source_id: &str, json_bytes: &[u8]) -> Vec<CountRecord> {
    let resp: TelraamApiResponse = match serde_json::from_slice(json_bytes) {
        Ok(r) => r,
        Err(e) => {
            web_sys::console::error_1(&format!("Telraam API parse failed: {:?}", e).into());
            return vec![];
        }
    };

    let emit_dirs = telraam_annotation(source_id).is_some();
    let atob_id   = format!("{}-atob", source_id);
    let btoa_id   = format!("{}-btoa", source_id);

    let mut records = Vec::new();
    for hour in resp.report {
        // Telraam serializes UTC timestamps as "YYYY-MM-DD HH:MM:SSZ".
        // Rewrite the space to a T so chrono's RFC 3339 parser accepts it.
        let iso = hour.date.replacen(' ', "T", 1);
        let Ok(ts) = DateTime::parse_from_rfc3339(&iso) else { continue };
        let ts = ts.with_timezone(&Utc);

        for (modality, count) in [
            (Modality::Bikes,       hour.bike),
            (Modality::Pedestrians, hour.pedestrian),
            (Modality::Cars,        hour.car),
            (Modality::Trucks,      hour.heavy),
        ] {
            if let Some(c) = count {
                records.push(CountRecord { timestamp: ts, modality, count: c,
                                           source_id: source_id.to_string() });
            }
        }

        if emit_dirs {
            for (sid, vals) in [
                (atob_id.as_str(), [hour.bike_lft, hour.pedestrian_lft, hour.car_lft, hour.heavy_lft]),
                (btoa_id.as_str(), [hour.bike_rgt, hour.pedestrian_rgt, hour.car_rgt, hour.heavy_rgt]),
            ] {
                for (modality, v) in [
                    (Modality::Bikes,       vals[0]),
                    (Modality::Pedestrians, vals[1]),
                    (Modality::Cars,        vals[2]),
                    (Modality::Trucks,      vals[3]),
                ] {
                    if let Some(c) = v {
                        records.push(CountRecord { timestamp: ts, modality, count: c,
                                                   source_id: sid.to_string() });
                    }
                }
            }
        }
    }
    records
}

/// Fetch a cron-written Telraam API JSON snapshot and parse it. A 404 is
/// treated as "cron hasn't produced one yet" and returns an empty record
/// set (no error), so the page still works on first deploy or in dev.
///
/// Also returns the response's `Last-Modified` time when available, so the
/// caller can display when cron last refreshed the snapshot — this is the
/// "last successful API call" signal, independent of whether the sensor
/// itself produced any new rows.
pub async fn fetch_telraam_api(source_id: &str, url: &str)
    -> Result<(Vec<CountRecord>, Option<DateTime<Utc>>), String>
{
    let resp = gloo_net::http::Request::get(url)
        .send().await
        .map_err(|e| format!("Fetch error: {:?}", e))?;
    if resp.status() == 404 { return Ok((vec![], None)); }
    if !resp.ok() { return Err(format!("HTTP {}", resp.status())); }
    let last_modified = resp.headers().get("last-modified")
        .and_then(|s| DateTime::parse_from_rfc2822(&s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let bytes = resp.binary().await.map_err(|e| format!("Binary error: {:?}", e))?;
    Ok((parse_telraam_api(source_id, &bytes), last_modified))
}

/// Parse a CDN-NDG access-to-information eco-counter Excel export.
///
/// Layout (ad hoc, set by the borough):
///   Row 1 — period banner ("Période ... → ...")
///   Row 2 — blank
///   Row 3 — column headers: A=Time, B=total cyclists, C=eastbound (IN_est),
///           D=westbound (OUT_ouest), E=motor vehicles, F=grand total
///   Rows 4..N — hourly data; Time is an Excel datetime cell in Montreal
///           local time (DST-aware UTC conversion via chrono-tz).
///   Trailing footer — either a "Total" row (string time cell, skipped because
///           it fails datetime parsing) or a partial last-hour row with a valid
///           timestamp but blank count cells (skipped via the all-blank guard).
///
/// Emits records under the source's id (totals) and `<id>-east` / `<id>-west`
/// (directionals).  Bike-only — the motor-vehicle column is intentionally
/// ignored even though the file carries it.
pub fn parse_cdn_ndg_excel(source_id: &str, bytes: &[u8]) -> Vec<CountRecord> {
    use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};

    let cursor = Cursor::new(bytes.to_vec());
    let mut wb: Xlsx<_> = match open_workbook_from_rs(cursor) {
        Ok(w) => w,
        Err(e) => {
            web_sys::console::error_1(&format!("CDN-NDG Excel open failed: {:?}", e).into());
            return vec![];
        }
    };

    let sheet_names = wb.sheet_names().to_vec();
    let Some(sheet) = sheet_names.first().cloned() else { return vec![] };
    let range = match wb.worksheet_range(&sheet) {
        Ok(r) => r,
        Err(e) => {
            web_sys::console::error_1(&format!("CDN-NDG sheet read failed: {:?}", e).into());
            return vec![];
        }
    };

    fn cell_str(c: &Data) -> Option<&str> {
        match c {
            Data::String(s) => Some(s.trim()),
            _ => None,
        }
    }
    fn cell_f64(c: &Data) -> f64 {
        match c {
            Data::Float(f) => *f,
            Data::Int(i)   => *i as f64,
            Data::Bool(b)  => if *b { 1.0 } else { 0.0 },
            _ => 0.0,
        }
    }
    fn extract_dt(c: &Data) -> Option<chrono::NaiveDateTime> {
        match c {
            Data::DateTime(edt) => edt.as_datetime(),
            Data::DateTimeIso(s) => chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()
                .or_else(|| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").ok()),
            Data::String(s) => chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok()
                .or_else(|| chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M").ok()),
            _ => None,
        }
    }
    fn val_at(row: &[Data], col: Option<usize>) -> f64 {
        col.and_then(|c| row.get(c)).map(cell_f64).unwrap_or(0.0)
    }
    // A cell is blank if it is missing or holds Data::Empty.
    fn is_blank(row: &[Data], col: Option<usize>) -> bool {
        !matches!(col.and_then(|c| row.get(c)), Some(c) if !matches!(c, Data::Empty))
    }

    let mut rows_iter = range.rows();
    // Banner + blank.
    let _ = rows_iter.next();
    let _ = rows_iter.next();
    let Some(header) = rows_iter.next() else { return vec![] };

    let find = |name: &str| -> Option<usize> {
        header.iter().position(|c| {
            cell_str(c).map_or(false, |s| s.eq_ignore_ascii_case(name))
        })
    };

    let dt_col         = find("time").unwrap_or(0);
    let bike_total_col = find("rue terrebonne cyclist");
    let bike_east_col  = find("rue terrebonne cyclist in_est");
    let bike_west_col  = find("rue terrebonne cyclist out_ouest");

    let east_id = format!("{}-east", source_id);
    let west_id = format!("{}-west", source_id);

    let mut records = Vec::new();
    for row in rows_iter {
        let Some(ndt) = row.get(dt_col).and_then(extract_dt) else { continue };
        // Skip a trailing partial row (valid timestamp, no counts yet).
        if is_blank(row, bike_total_col)
            && is_blank(row, bike_east_col)
            && is_blank(row, bike_west_col)
        {
            continue;
        }
        let ts = match MontrealTz.from_local_datetime(&ndt) {
            LocalResult::Single(dt)       => dt.with_timezone(&Utc),
            LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
            LocalResult::None             => continue,
        };

        records.push(CountRecord {
            timestamp: ts, modality: Modality::Bikes,
            count: val_at(row, bike_total_col),
            source_id: source_id.to_string(),
        });
        records.push(CountRecord {
            timestamp: ts, modality: Modality::Bikes,
            count: val_at(row, bike_east_col),
            source_id: east_id.clone(),
        });
        records.push(CountRecord {
            timestamp: ts, modality: Modality::Bikes,
            count: val_at(row, bike_west_col),
            source_id: west_id.clone(),
        });
    }

    records
}

/// Fetch a CDN-NDG Excel file from a URL and parse it.
pub async fn fetch_cdn_ndg_excel(source_id: &str, url: &str) -> Result<Vec<CountRecord>, String> {
    let resp = gloo_net::http::Request::get(url)
        .send().await
        .map_err(|e| format!("Fetch error: {:?}", e))?;
    if !resp.ok() { return Err(format!("HTTP {}", resp.status())); }
    let bytes = resp.binary().await.map_err(|e| format!("Binary error: {:?}", e))?;
    Ok(parse_cdn_ndg_excel(source_id, &bytes))
}

// ── Aggregation ──────────────────────────────────────────────────────────────

pub fn aggregate(
    records: &[CountRecord],
    modality: Modality,
    resolution: Resolution,
    source_id: Option<&str>,
) -> Vec<(DateTime<Utc>, f64)> {
    let mut buckets: BTreeMap<i64, f64> = BTreeMap::new();
    let mut min_ts: Option<DateTime<Utc>> = None;
    let mut max_ts: Option<DateTime<Utc>> = None;
    for rec in records {
        if rec.modality != modality { continue; }
        if source_id.map_or(false, |id| rec.source_id != id) { continue; }
        min_ts = Some(min_ts.map_or(rec.timestamp, |m| m.min(rec.timestamp)));
        max_ts = Some(max_ts.map_or(rec.timestamp, |m| m.max(rec.timestamp)));
        let key = bucket_key(rec.timestamp, resolution);
        *buckets.entry(key).or_insert(0.0) += rec.count;
    }
    let mut out: Vec<(DateTime<Utc>, f64)> = buckets.into_iter()
        .filter_map(|(k, v)| DateTime::from_timestamp(k, 0).map(|dt| (dt, v)))
        .collect();

    // Trim a leading or trailing partial bucket for Week / Month resolutions
    // so the chart doesn't show a half-formed sum that reads as a traffic
    // drop. A bucket is "complete" iff the data extent reaches its first day
    // (leading) or last day (trailing). Hour and Day buckets are atomic.
    if matches!(resolution, Resolution::Week | Resolution::Month) {
        if let (Some(min), Some(max)) = (min_ts, max_ts) {
            if let Some((b_start, _)) = out.first().copied() {
                if min.date_naive() != b_start.date_naive() {
                    out.remove(0);
                }
            }
            if let Some((b_start, _)) = out.last().copied() {
                if max.date_naive() != bucket_last_day(b_start, resolution) {
                    out.pop();
                }
            }
        }
    }

    out
}

fn bucket_last_day(b_start: DateTime<Utc>, res: Resolution) -> chrono::NaiveDate {
    match res {
        Resolution::Week => b_start.date_naive() + chrono::Duration::days(6),
        Resolution::Month => {
            let nd = b_start.date_naive();
            let (y, m) = if nd.month() == 12 {
                (nd.year() + 1, 1)
            } else {
                (nd.year(), nd.month() + 1)
            };
            chrono::NaiveDate::from_ymd_opt(y, m, 1).unwrap() - chrono::Duration::days(1)
        }
        // Hour and Day buckets are atomic — this helper is only called for
        // Week / Month, but return something sensible for completeness.
        Resolution::Hour | Resolution::Day => b_start.date_naive(),
    }
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
