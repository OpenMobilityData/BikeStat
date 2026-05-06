use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Modality {
    Bikes,
    Pedestrians,
    Cars,
    Trucks,
}

impl Modality {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Bikes       => "Bikes",
            Self::Pedestrians => "Pedestrians",
            Self::Cars        => "Cars",
            Self::Trucks      => "Trucks",
        }
    }

    pub fn color(&self) -> &'static str {
        match self {
            Self::Bikes       => "#e94560",
            Self::Pedestrians => "#f5a623",
            Self::Cars        => "#4a9eff",
            Self::Trucks      => "#7ed321",
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
        }
    }

    pub fn all() -> &'static [Modality] {
        &[Self::Bikes, Self::Pedestrians, Self::Cars, Self::Trucks]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution { Hour, Day, Week, Month }

/// How the chart plots time. `Linear` is the default — one continuous time
/// axis. `YearOnYear` collapses every 12-month block of data onto a shared
/// 12-month axis so seasonal patterns can be compared across years.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode { Linear, YearOnYear }

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
    /// Eco-counter Excel exports obtained via access-to-information requests
    /// to the CDN-NDG borough.  Layout is ad hoc (banner row, blank, header,
    /// hourly data, "Total" footer).  `file_urls`: paths relative to app root,
    /// one per quarterly batch, e.g.
    /// `"data/cdn-ndg/terrebonne-kensington/2025-07-26_2025-11-15.xlsx"`.
    CdnNdgExcel { file_urls: Vec<String> },
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
    /// Shared key for sources that belong to the same physical location group
    /// (e.g. directionals + total for one intersection).  The sidebar wraps
    /// consecutive sources with the same non-None group into a visual cluster.
    pub group: Option<String>,
}
