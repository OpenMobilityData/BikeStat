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
            Self::Bikes => "Bikes",
            Self::Pedestrians => "Pedestrians",
            Self::Cars => "Cars",
            Self::Trucks => "Trucks",
            Self::Motorcycles => "Motorcycles",
        }
    }

    pub fn color(&self) -> &'static str {
        match self {
            Self::Bikes => "#e94560",
            Self::Pedestrians => "#f5a623",
            Self::Cars => "#4a9eff",
            Self::Trucks => "#7ed321",
            Self::Motorcycles => "#bd10e0",
        }
    }

    pub fn all() -> &'static [Modality] {
        &[
            Self::Bikes,
            Self::Pedestrians,
            Self::Cars,
            Self::Trucks,
            Self::Motorcycles,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    Hour,
    Day,
    Week,
    Month,
}

impl Resolution {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Hour => "Hour",
            Self::Day => "Day",
            Self::Week => "Week",
            Self::Month => "Month",
        }
    }
}

/// A single count record from a data source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountRecord {
    pub timestamp: DateTime<Utc>,
    pub modality: Modality,
    pub count: f64,
    pub source_id: String,
}

/// Geographic location of a counting station.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

/// Metadata about a data source/counting station.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSource {
    pub id: String,
    pub name: String,
    pub location: LatLon,
    pub modalities: Vec<Modality>,
    pub earliest: DateTime<Utc>,
    pub latest: DateTime<Utc>,
    pub color: String,
}

/// Aggregated data point after grouping by resolution.
#[derive(Debug, Clone)]
pub struct AggPoint {
    pub bucket_start: DateTime<Utc>,
    pub modality: Modality,
    pub source_id: String,
    pub total: f64,
}
