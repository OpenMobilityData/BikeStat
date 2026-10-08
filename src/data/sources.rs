use chrono::{DateTime, TimeZone, Utc};
use crate::data::types::{DataSource, LatLon, LoaderType, Modality};

/// VdM cyclistes CSV, served same-origin.  An hourly cron job on the server
/// (`scripts/refresh-vdm.sh`) rebuilds it from the city's per-year
/// `cyclistes_<year>.csv` files, pre-filtered to the catalogued streets.  For
/// local dev, populate `static/data/cyclistes.csv` once with:
///
/// ```bash
/// BIKESTAT_DATA_DIR=static/data ./scripts/refresh-vdm.sh
/// ```
///
/// When built with `--features unfiltered`, the loader instead pulls an
/// unfiltered year from `data/cyclistes-all.csv` (see the README).
#[cfg(not(feature = "unfiltered"))]
pub const MONTREAL_CYCLISTES_URL: &str = "data/cyclistes.csv";
#[cfg(feature = "unfiltered")]
pub const MONTREAL_CYCLISTES_URL: &str = "data/cyclistes-all.csv";

/// Whitelist of Montreal locations to include, matched case-insensitively as
/// substrings against (rue_1, rue_2).  `None` = include all locations.
///
/// The order of entries here also drives the sidebar order of the matched VdM
/// intersections.  Each intersection's directional rows and synthesised Total
/// stay clustered together; intersections that don't match any entry (only
/// possible when this filter is `None`) fall back to alphabetical order.
///
/// "carie" matches "Décarie" without requiring exact accent handling.
#[cfg(not(feature = "unfiltered"))]
pub const MONTREAL_LOCATION_FILTER: Option<&[(&str, &str)]> = Some(&[
    ("Girouard", "Terrebonne"),
    ("Bourret",  "carie"),       // Bourret @ Décarie (any direction)
]);
#[cfg(feature = "unfiltered")]
pub const MONTREAL_LOCATION_FILTER: Option<&[(&str, &str)]> = None;

/// VdM cyclistes detector feeds to fold into another feed, as
/// `((instance, direction), (instance, direction))`.  Applied before
/// grouping, so the alias's rows land in the target source.
///
/// det-00118-03 (no direction) is the Girouard northbound camera under a
/// different id: it reports alone until 2025-10-29 17:00, det-00118-02 "Nord"
/// then takes over until 2026-06-27 20:00, and det-00118-03 resumes.  The two
/// share only those hand-over hours (each a partial hour, so summing them is
/// correct) and sit ~15 m apart.  Confirmed on site: the Nord/Sud cameras
/// face Girouard; det-00118-01 "Ouest" faces westbound Terrebonne.
pub const MONTREAL_INSTANCE_ALIASES: &[((&str, &str), (&str, &str))] = &[
    (("det-00118-03", ""), ("det-00118-02", "Nord")),
];

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
    /// Compact tag for the monitored street, used in the freshness chips
    /// where horizontal space is tight (e.g. `"TB"` for Terrebonne, `"NDG"`
    /// for Notre-Dame-de-Grâce). Combined with the cross-street parsed from
    /// `display_name` to form a chip label like `"TB@King Edward"`.
    pub street_abbrev: &'static str,
    /// Label for the A→B travel direction.
    /// Set to a compass direction once confirmed from the Telraam segment map,
    /// e.g. `"Eastbound"` or `"→ Snowdon"`.
    pub dir_a_to_b: &'static str,
    /// Label for the B→A travel direction.
    pub dir_b_to_a: &'static str,
    /// Telraam-API segment id (a.k.a. "location id" on telraam.net), distinct
    /// from the legacy directory id used as the source key. Used to build
    /// attribution links to the segment's public Telraam page.
    pub api_id: &'static str,
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
            display_name: "Telraam: Terrebonne @ King Edward",
            street_abbrev: "TB",
            dir_a_to_b: "Eastbound",
            dir_b_to_a: "Westbound",
            api_id: "9000007290",
        },
    ),
    (
        "telraam-10045",
        TelraamAnnotation {
            display_name: "Telraam: Terrebonne @ Royal",
            street_abbrev: "TB",
            // Note: A/B orientation is reversed relative to the King Edward
            // segment — confirmed against the Telraam segment map.
            dir_a_to_b: "Westbound",
            dir_b_to_a: "Eastbound",
            api_id: "9000007489",
        },
    ),
    (
        "telraam-9000011055",
        TelraamAnnotation {
            display_name: "Telraam: NDG @ Hampton",
            street_abbrev: "NDG",
            dir_a_to_b: "Eastbound",
            dir_b_to_a: "Westbound",
            api_id: "9000011055",
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
        LatLon { lat: 45.46392, lon: -73.63756 },
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        vec![
            "data/telraam/9794/2024.xlsx".into(),
            "data/telraam/9794/2025.xlsx".into(),
            "data/telraam/9794/2026.xlsx".into(),
        ],
        5, // SOURCE_COLORS index for the total; +1 → A→B, +2 → B→A
    );

    push_telraam_segment(
        &mut out,
        "telraam-10045",
        LatLon { lat: 45.47303, lon: -73.62965 },
        Utc.with_ymd_and_hms(2024, 9, 24, 0, 0, 0).unwrap(),
        vec![
            "data/telraam/10045/2024.xlsx".into(),
            "data/telraam/10045/2025.xlsx".into(),
            "data/telraam/10045/2026.xlsx".into(),
        ],
        8, // base_color_idx: +1 → A→B, +2 → B→A
    );

    // NDG @ Hampton — installed 2026-04-16, basic subscription (no historical
    // xlsx export available, so file_urls is empty; the cron-written API
    // snapshot is the sole record source). Coordinates are the midpoint of
    // the Telraam segment line. base_color_idx 14 wraps around the 15-slot
    // SOURCE_COLORS palette (Telraam used 5, 8; CDN-NDG used 11), so the
    // directional sub-sources land on indices 0 and 1 — visually distinct
    // from existing Telraam/CDN-NDG markers.
    push_telraam_segment(
        &mut out,
        "telraam-9000011055",
        LatLon { lat: 45.46932, lon: -73.62251 },
        Utc.with_ymd_and_hms(2026, 4, 16, 0, 0, 0).unwrap(),
        vec![],
        14, // base_color_idx: +1 → A→B, +2 → B→A
    );

    out
}

// ── CDN-NDG access-to-info eco-counter sources ───────────────────────────────

/// Pre-configured CDN-NDG borough eco-counter sources.
///
/// These are bike-only counters whose data arrives quarterly via access-to-
/// information requests.  To integrate a new batch:
/// 1. Drop the Excel file in `static/data/cdn-ndg/<location>/` using an
///    ISO date-range filename (e.g. `2025-11-15_2026-02-15.xlsx`).
/// 2. Add its path to the corresponding `file_urls` list below.
pub fn cdn_ndg_sources() -> Vec<DataSource> {
    let mut out = Vec::new();

    push_cdn_ndg_counter(
        &mut out,
        "cdnndg-terrebonne-kensington",
        "CDN-NDG: Terrebonne @ Kensington",
        LatLon { lat: 45.47022, lon: -73.63204 },
        Utc.with_ymd_and_hms(2025, 7, 26, 0, 0, 0).unwrap(),
        vec![
            "data/cdn-ndg/terrebonne-kensington/2025-07-26_2026-05-11.xlsx".into(),
        ],
        11, // base_color_idx; +1 → east, +2 → west.  Telraam used 5 and 8.
    );

    out
}

// ── VdM eco-counter sources ──────────────────────────────────────────────────

/// First year of VdM eco-counter data to load.  The client requests
/// `vdm_eco_url(y)` for every year from here through the current one.
pub const VDM_ECO_FIRST_YEAR: i32 = 2024;

/// Per-year eco-counter CSV, pre-filtered to the counters below and served
/// same-origin.  An hourly cron job (`scripts/refresh-vdm-eco.sh`) writes
/// these; for local dev run that script once with
/// `BIKESTAT_DATA_DIR=static/data`.
pub fn vdm_eco_url(year: i32) -> String {
    format!("data/vdm-eco/{}.csv", year)
}

/// One physical VdM eco-counter site.  A site with a single counter yields
/// one source whose id is the site id.  A site with several co-located
/// counters yields one sub-source per counter (`<site id>-<id_compteur>`)
/// plus a synthesised Total under the site id, matching the VdM cyclistes
/// convention of directionals followed by `(Total)`.
pub struct VdmEcoSite {
    /// Source id of the site (the Total, for multi-counter sites).  Must
    /// start with `mtl-` so the site shows in the VdM freshness tooltip.
    pub id: &'static str,
    pub name: &'static str,
    pub location: LatLon,
    /// `(id_compteur, label)` pairs.  The label is only used for
    /// multi-counter sites, as the parenthesised sub-source suffix.
    pub counters: &'static [(&'static str, &'static str)],
    pub earliest: (i32, u32, u32),
    /// `SOURCE_COLORS` index for the site (Total); sub-sources take +1, +2, …
    pub base_color_idx: usize,
}

/// Catalogued VdM eco-counter sites, in sidebar order.  Every `id_compteur`
/// here must also be listed in `COUNTER_IDS` in `scripts/refresh-vdm-eco.sh`
/// (the server-side pre-filter).  Locations and ids come from the dataset's
/// `localisations_globale.csv`.
pub static VDM_ECO_SITES: &[VdmEcoSite] = &[
    VdmEcoSite {
        id: "mtl-eco-maisonneuve-marcil",
        name: "VdM: Maisonneuve @ Marcil",
        location: LatLon { lat: 45.470493, lon: -73.609566 },
        counters: &[("100011783", "")],
        earliest: (2024, 1, 1),
        base_color_idx: 2,
    },
    // City of Westmount.  Two counters share one point on Av Westmount by
    // the park; the city's Power BI map shows them as a single "Av Lansdowne"
    // site whose volume is the sum of both.
    VdmEcoSite {
        id: "mtl-eco-westmount-lansdowne",
        name: "VdM: Westmount / Lansdowne",
        location: LatLon { lat: 45.486020, lon: -73.605780 },
        counters: &[("100060991", "Av Westmount"), ("100060992", "Av Lansdowne")],
        earliest: (2026, 1, 1),
        base_color_idx: 4,
    },
    VdmEcoSite {
        id: "mtl-eco-westmount-cote-st-antoine",
        name: "VdM: Westmount / Côte-St-Antoine",
        location: LatLon { lat: 45.483957, lon: -73.603037 },
        counters: &[("100061090", "")],
        earliest: (2026, 1, 1),
        base_color_idx: 7,
    },
];

/// Source id that receives a counter's records, and the site Total id it
/// also feeds when the site has several counters.
pub fn vdm_eco_counter_target(id_compteur: &str) -> Option<(String, Option<&'static str>)> {
    VDM_ECO_SITES.iter().find_map(|site| {
        site.counters.iter().any(|(c, _)| *c == id_compteur).then(|| {
            if site.counters.len() == 1 {
                (site.id.to_string(), None)
            } else {
                (format!("{}-{}", site.id, id_compteur), Some(site.id))
            }
        })
    })
}

/// Pre-configured VdM eco-counter sources.  Records for all of them arrive
/// together from the per-year CSVs, so the sources are `Discovered`.
pub fn vdm_eco_sources() -> Vec<DataSource> {
    let mut out = Vec::new();
    for site in VDM_ECO_SITES {
        let (y, m, d) = site.earliest;
        let earliest = Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap();
        let color = |offset: usize|
            SOURCE_COLORS[(site.base_color_idx + offset) % SOURCE_COLORS.len()].to_string();
        let source = |id: String, name: String, color: String| DataSource {
            id, name,
            location: site.location.clone(),
            modalities: vec![Modality::Bikes],
            earliest,
            latest: Utc::now(),
            color,
            loader_type: LoaderType::Discovered,
            group: Some(site.id.to_string()),
        };

        if site.counters.len() == 1 {
            out.push(source(site.id.into(), site.name.into(), color(0)));
            continue;
        }
        for (i, (id_compteur, label)) in site.counters.iter().enumerate() {
            out.push(source(
                format!("{}-{}", site.id, id_compteur),
                format!("{} ({})", site.name, label),
                color(i + 1),
            ));
        }
        out.push(source(site.id.into(), format!("{} (Total)", site.name), color(0)));
    }
    out
}

/// Build and push the directional + total entries for a CDN-NDG counter.
///
/// The directional sub-sources share the same `group` key as the total source
/// (which equals the total source's id, by the same convention used elsewhere).
/// `parse_cdn_ndg_excel` emits records under the `-east` / `-west` id suffixes.
fn push_cdn_ndg_counter(
    out: &mut Vec<DataSource>,
    source_id: &str,
    display_name: &str,
    location: LatLon,
    earliest: DateTime<Utc>,
    file_urls: Vec<String>,
    base_color_idx: usize,
) {
    let (lat, lon) = (location.lat, location.lon);
    let mods = || vec![Modality::Bikes];

    // Directionals first so they appear above the total in the sidebar cluster.
    for (suffix, dir_label, color_offset) in [
        ("east", "Eastbound", 1usize),
        ("west", "Westbound", 2usize),
    ] {
        out.push(DataSource {
            id: format!("{}-{}", source_id, suffix),
            name: format!("{} — ({})", display_name, dir_label),
            location: LatLon { lat, lon },
            modalities: mods(),
            earliest,
            latest: Utc::now(),
            color: SOURCE_COLORS[(base_color_idx + color_offset) % SOURCE_COLORS.len()].into(),
            loader_type: LoaderType::Discovered,
            group: Some(source_id.to_string()),
        });
    }

    out.push(DataSource {
        id: source_id.into(),
        name: format!("{} — Total", display_name),
        location: LatLon { lat, lon },
        modalities: mods(),
        earliest,
        latest: Utc::now(),
        color: SOURCE_COLORS[base_color_idx % SOURCE_COLORS.len()].into(),
        loader_type: LoaderType::CdnNdgExcel { file_urls },
        group: Some(source_id.to_string()),
    });
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
