use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Modality {
    Bikes,
    Pedestrians,
    Cars,
    Trucks,
    Motorcycles,
}

impl Modality {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Bikes       => "Bikes",
            Self::Pedestrians => "Pedestrians",
            Self::Cars        => "Cars",
            Self::Trucks      => "Trucks",
            Self::Motorcycles => "Motorcycles",
        }
    }

    pub fn color(&self) -> &'static str {
        match self {
            Self::Bikes       => "#e94560",
            Self::Pedestrians => "#f5a623",
            Self::Cars        => "#4a9eff",
            Self::Trucks      => "#7ed321",
            Self::Motorcycles => "#bd10e0",
        }
    }

    /// SVG `stroke-dasharray` value for this modality.
    /// `None` = solid line (Bikes, the primary modality).
    pub fn stroke_dasharray(&self) -> Option<&'static str> {
        match self {
            Self::Bikes       => None,
            Self::Pedestrians => Some("5 3"),
            Self::Cars        => Some("2 3"),
            Self::Trucks      => Some("8 3 2 3"),
            Self::Motorcycles => Some("12 4"),
        }
    }

    pub fn all() -> &'static [Modality] {
        &[Self::Bikes, Self::Pedestrians, Self::Cars, Self::Trucks, Self::Motorcycles]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution { Hour, Day, Week, Month }

impl Resolution {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Hour  => "Hour",
            Self::Day   => "Day",
            Self::Week  => "Week",
            Self::Month => "Month",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountRecord {
    pub timestamp: DateTime<Utc>,
    pub modality: Modality,
    pub count: f64,
    pub source_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

/// Describes how a source's records are (or will be) loaded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LoaderType {
    /// Records already in memory (e.g. discovered from a multi-source CSV feed).
    Discovered,
    /// Telraam S2 Excel exports served as static files.
    /// `file_urls`: paths relative to app root, one per year,
    /// e.g. `"data/telraam/seg-12345/2024.xlsx"`.
    TelraamExcel {
        segment_id: String,
        file_urls: Vec<String>,
    },
    /// Telraam S2 live API (requires an API key configured server-side).
    TelraamApi { segment_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSource {
    pub id: String,
    pub name: String,
    pub location: LatLon,
    pub modalities: Vec<Modality>,
    pub earliest: DateTime<Utc>,
    pub latest: DateTime<Utc>,
    pub color: String,
    pub loader_type: LoaderType,
}
