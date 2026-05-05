use chrono::{TimeZone, Utc};
use crate::data::types::{DataSource, LatLon, Modality};

/// Returns the hard-coded catalogue of known data sources.
/// Each source will have a corresponding loader implementation.
pub fn catalogue() -> Vec<DataSource> {
    vec![
        DataSource {
            id: "eco-totem-comox".into(),
            name: "Eco-Totem: Comox St".into(),
            location: LatLon { lat: 49.2827, lon: -123.1207 },
            modalities: vec![Modality::Bikes, Modality::Pedestrians],
            earliest: Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap(),
            latest:   Utc.with_ymd_and_hms(2025, 12, 31, 23, 0, 0).unwrap(),
            color: "#e94560".into(),
        },
        DataSource {
            id: "eco-totem-burrard".into(),
            name: "Eco-Totem: Burrard Bridge".into(),
            location: LatLon { lat: 49.2763, lon: -123.1386 },
            modalities: vec![Modality::Bikes, Modality::Pedestrians],
            earliest: Utc.with_ymd_and_hms(2019, 6, 1, 0, 0, 0).unwrap(),
            latest:   Utc.with_ymd_and_hms(2025, 12, 31, 23, 0, 0).unwrap(),
            color: "#4a9eff".into(),
        },
        DataSource {
            id: "inrix-main-broadway".into(),
            name: "INRIX: Main & Broadway".into(),
            location: LatLon { lat: 49.2635, lon: -123.1014 },
            modalities: vec![Modality::Bikes, Modality::Cars, Modality::Trucks],
            earliest: Utc.with_ymd_and_hms(2022, 1, 1, 0, 0, 0).unwrap(),
            latest:   Utc.with_ymd_and_hms(2025, 12, 31, 23, 0, 0).unwrap(),
            color: "#7ed321".into(),
        },
    ]
}
