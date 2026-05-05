use chrono::{TimeZone, Utc};
use crate::data::types::{DataSource, LatLon, LoaderType, Modality};

/// Montreal open data: all cyclist counters in a single CSV.
pub const MONTREAL_CYCLISTES_URL: &str = "https://donnees.montreal.ca/dataset/\
    142ff2e9-7d0a-47d6-b4f6-dfeb97041daf/resource/\
    a8e463ab-d334-4714-81d5-8da0310d80c0/download/cyclistes.csv";

/// Whitelist of Montreal locations to include, matched case-insensitively as
/// substrings against (rue_1, rue_2).  `None` = include all locations.
///
/// "carie" matches "Décarie" without requiring exact accent handling.
pub const MONTREAL_LOCATION_FILTER: Option<&[(&str, &str)]> = Some(&[
    ("Bourret",  "carie"),       // Bourret @ Décarie (any direction)
    ("Girouard", "Terrebonne"),
]);

/// Color palette cycled through when assigning colors to discovered sources.
pub const SOURCE_COLORS: &[&str] = &[
    "#e94560", "#4a9eff", "#7ed321", "#f5a623", "#bd10e0",
    "#50e3c2", "#ff6b6b", "#45b7d1", "#98d8c8", "#f7dc6f",
    "#bb8fce", "#85c1e9", "#82e0aa", "#f0b27a", "#aab7b8",
];

/// Pre-configured Telraam S2 sources.
///
/// Each entry maps directly to one (or more) Excel files served from
/// `static/data/telraam/<segment_id>/<year>.xlsx`.
///
/// Location coordinates are approximate; update from the Telraam API if needed.
pub fn telraam_sources() -> Vec<DataSource> {
    vec![
        DataSource {
            id:         "telraam-9794".into(),
            name:       "Telraam: Terrebonne (NDG)".into(),
            // Approximate location of the S2 counter on rue de Terrebonne
            location:   LatLon { lat: 45.471, lon: -73.609 },
            modalities: vec![
                Modality::Bikes,
                Modality::Pedestrians,
                Modality::Cars,
                Modality::Trucks,
            ],
            earliest: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            latest:   Utc::now(),
            color:    "#50e3c2".into(),
            loader_type: LoaderType::TelraamExcel {
                segment_id: "9794".into(),
                file_urls:  vec!["data/telraam/9794/2026.xlsx".into()],
            },
        },
    ]
}
