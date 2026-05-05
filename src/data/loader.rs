use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use crate::data::types::{CountRecord, Modality};

/// Parse a CSV from the Vancouver Open Data Eco-Totem format.
/// Expected columns: Date, Hour, Bike_Count, Pedestrian_Count  (header row required)
pub fn parse_eco_totem_csv(source_id: &str, csv_text: &str) -> Vec<CountRecord> {
    let mut records = Vec::new();
    let mut lines = csv_text.lines();
    let Some(header) = lines.next() else { return records };

    let cols: Vec<&str> = header.split(',').collect();
    let find = |name: &str| cols.iter().position(|&c| c.trim().eq_ignore_ascii_case(name));

    let Some(date_col) = find("date") else { return records };
    let Some(hour_col) = find("hour") else { return records };
    let bike_col = find("bike_count");
    let ped_col  = find("pedestrian_count");

    for line in lines {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() <= date_col.max(hour_col) { continue; }

        let date_str = fields[date_col].trim();
        let hour_str = fields[hour_col].trim();
        let Ok(hour): Result<u32, _> = hour_str.parse() else { continue };

        let datetime_str = format!("{} {:02}:00:00", date_str, hour);
        let Ok(ndt) = NaiveDateTime::parse_from_str(&datetime_str, "%Y-%m-%d %H:%M:%S") else {
            continue
        };
        let ts: DateTime<Utc> = Utc.from_utc_datetime(&ndt);

        let push = |records: &mut Vec<CountRecord>, modality: Modality, col: usize| {
            if let Ok(count) = fields.get(col).unwrap_or(&"").trim().parse::<f64>() {
                records.push(CountRecord { timestamp: ts, modality, count, source_id: source_id.to_string() });
            }
        };

        if let Some(c) = bike_col { push(&mut records, Modality::Bikes, c); }
        if let Some(c) = ped_col  { push(&mut records, Modality::Pedestrians, c); }
    }
    records
}

/// Aggregate raw records into buckets determined by resolution.
/// Returns (bucket_start_ms, total) pairs suitable for chart rendering.
pub fn aggregate(
    records: &[CountRecord],
    modality: Modality,
    resolution: crate::data::types::Resolution,
    source_id: Option<&str>,
) -> Vec<(DateTime<Utc>, f64)> {
    use std::collections::BTreeMap;
    use crate::data::types::Resolution;

    let filtered = records.iter().filter(|r| {
        r.modality == modality && source_id.map_or(true, |id| r.source_id == id)
    });

    let mut buckets: BTreeMap<i64, f64> = BTreeMap::new();
    for rec in filtered {
        let bucket = bucket_key(rec.timestamp, resolution);
        *buckets.entry(bucket).or_insert(0.0) += rec.count;
    }

    buckets.into_iter()
        .filter_map(|(k, v)| {
            DateTime::from_timestamp(k, 0).map(|dt| (dt, v))
        })
        .collect()
}

fn bucket_key(ts: DateTime<Utc>, res: crate::data::types::Resolution) -> i64 {
    use crate::data::types::Resolution;
    use chrono::{Datelike, Timelike};
    match res {
        Resolution::Hour  => ts.with_minute(0).unwrap().with_second(0).unwrap().timestamp(),
        Resolution::Day   => ts.date_naive().and_hms_opt(0,0,0).unwrap()
                               .and_utc().timestamp(),
        Resolution::Week  => {
            let dow = ts.weekday().num_days_from_monday();
            (ts.date_naive() - chrono::Duration::days(dow as i64))
                .and_hms_opt(0,0,0).unwrap().and_utc().timestamp()
        }
        Resolution::Month => {
            ts.date_naive().with_day(1).unwrap()
              .and_hms_opt(0,0,0).unwrap().and_utc().timestamp()
        }
    }
}
