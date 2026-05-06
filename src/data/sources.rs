use chrono::{DateTime, TimeZone, Utc};
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

// ── Telraam counter annotations ───────────────────────────────────────────────

/// Developer-provided annotation for a Telraam counter segment.
///
/// Registering an annotation for a segment:
/// 1. Adds a better display name to the sidebar entry.
/// 2. Creates two additional per-direction sub-sources alongside the total.
///
/// To add or update annotations, edit `TELRAAM_ANNOTATIONS` below.
pub struct TelraamAnnotation {
    /// Human-readable display name shown in the sidebar.
    /// Should identify the street and cross-street, e.g.
    /// `"Telraam: Terrebonne @ de Courtrai (NDG)"`.
    pub display_name: &'static str,
    /// Label for the A→B travel direction.
    /// Set to a compass direction once confirmed from the Telraam segment map,
    /// e.g. `"Eastbound"` or `"→ Snowdon"`.
    pub dir_a_to_b: &'static str,
    /// Label for the B→A travel direction.
    pub dir_b_to_a: &'static str,
}

/// Lookup table of developer-provided annotations keyed by Telraam source ID.
///
/// Each entry corresponds to one physical counter.  Adding a new counter here
/// (with matching `push_telraam_segment` call in `telraam_sources`) is all
/// that is required to enable per-direction sub-sources for that counter.
static TELRAAM_ANNOTATIONS: &[(&str, TelraamAnnotation)] = &[
    (
        "telraam-9794",
        TelraamAnnotation {
            display_name: "Telraam: Terrebonne (NDG)",
            // TODO: confirm actual compass orientation from the Telraam segment map
            dir_a_to_b: "A→B",
            dir_b_to_a: "B→A",
        },
    ),
];

/// Look up the developer annotation for a Telraam source ID.
/// Returns `None` if no annotation has been registered for this segment.
pub fn telraam_annotation(source_id: &str) -> Option<&'static TelraamAnnotation> {
    TELRAAM_ANNOTATIONS
        .iter()
        .find(|(id, _)| *id == source_id)
        .map(|(_, ann)| ann)
}

// ── Telraam source catalogue ──────────────────────────────────────────────────

/// Pre-configured Telraam S2 sources.
///
/// Each call to `push_telraam_segment` registers:
/// - one total `DataSource` (LoaderType::TelraamExcel)
/// - if an annotation is registered: two directional sub-sources
///   (LoaderType::Discovered; records come from the same Excel fetch)
///
/// `base_color_idx` is the index into `SOURCE_COLORS` for the total source.
/// The two directional sources consume the next two slots, so leave a gap of 3
/// between segments.
pub fn telraam_sources() -> Vec<DataSource> {
    let mut out = Vec::new();

    push_telraam_segment(
        &mut out,
        "telraam-9794",
        LatLon { lat: 45.471, lon: -73.609 },
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        vec![
            "data/telraam/9794/2024.xlsx".into(),
            "data/telraam/9794/2025.xlsx".into(),
            "data/telraam/9794/2026.xlsx".into(),
        ],
        5, // SOURCE_COLORS index for the total; +1 → A→B, +2 → B→A
    );

    // To add a second counter:
    //   1. Add its annotation to TELRAAM_ANNOTATIONS above.
    //   2. Copy the push_telraam_segment call, update the segment ID,
    //      location, earliest date, file_urls, and base_color_idx (step of 3).

    out
}

/// Build and push a Telraam total source and, when an annotation is registered,
/// two per-direction sub-sources into `out`.
fn push_telraam_segment(
    out: &mut Vec<DataSource>,
    source_id: &str,
    location: LatLon,
    earliest: DateTime<Utc>,
    file_urls: Vec<String>,
    base_color_idx: usize,
) {
    let ann = telraam_annotation(source_id);
    let seg = source_id.strip_prefix("telraam-").unwrap_or(source_id);
    let display_name: String = ann
        .map(|a| a.display_name.to_string())
        .unwrap_or_else(|| format!("Telraam: {}", seg));
    let (lat, lon) = (location.lat, location.lon);
    let mods = || vec![Modality::Bikes, Modality::Pedestrians, Modality::Cars, Modality::Trucks];

    // Directional sub-sources first (when annotation present), then total —
    // consistent with the Montreal pattern of directionals before Total.
    if let Some(ann) = ann {
        for (suffix, dir_label, color_offset) in [
            ("atob", ann.dir_a_to_b, 1usize),
            ("btoa", ann.dir_b_to_a, 2usize),
        ] {
            out.push(DataSource {
                id: format!("{}-{}", source_id, suffix),
                name: format!("{} — ({})", display_name, dir_label),
                location: LatLon { lat, lon },
                modalities: mods(),
                earliest,
                latest: Utc::now(),
                color: SOURCE_COLORS[(base_color_idx + color_offset) % SOURCE_COLORS.len()]
                    .into(),
                loader_type: LoaderType::Discovered,
                group: Some(source_id.to_string()),
            });
        }
    }

    // Total source — fetched from the Excel file(s)
    let total_name = if ann.is_some() {
        format!("{} — Total", display_name)
    } else {
        display_name.clone()
    };
    out.push(DataSource {
        id: source_id.into(),
        name: total_name,
        location: LatLon { lat, lon },
        modalities: mods(),
        earliest,
        latest: Utc::now(),
        color: SOURCE_COLORS[base_color_idx % SOURCE_COLORS.len()].into(),
        loader_type: LoaderType::TelraamExcel {
            segment_id: seg.into(),
            file_urls,
        },
        group: ann.map(|_| source_id.to_string()),
    });
}
