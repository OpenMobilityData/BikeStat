//! In-app i18n for English / French.
//!
//! Every user-facing string lives as a `&'static str` field on `T`, with
//! one `const` instance per language (`EN`, `FR`). Lookup is
//! `lang.t().field_name` — typos and missing fields are compile errors.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang { En, Fr }

impl Lang {
    const STORAGE_KEY: &'static str = "bikestat-lang";

    /// Initial language: stored preference if set, else navigator.language
    /// (anything starting with "fr" → French), else English.
    pub fn from_browser() -> Self {
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                if let Ok(Some(s)) = storage.get_item(Self::STORAGE_KEY) {
                    if s == "fr" { return Self::Fr; }
                    if s == "en" { return Self::En; }
                }
            }
            let nav_lang = window.navigator().language().unwrap_or_default();
            if nav_lang.to_lowercase().starts_with("fr") { return Self::Fr; }
        }
        Self::En
    }

    pub fn store(self) {
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.set_item(Self::STORAGE_KEY, self.code());
            }
        }
    }

    pub fn code(self) -> &'static str {
        match self { Self::En => "en", Self::Fr => "fr" }
    }

    pub fn other(self) -> Self {
        match self { Self::En => Self::Fr, Self::Fr => Self::En }
    }

    /// Two-letter label for the toggle button.
    pub fn short_label(self) -> &'static str {
        match self { Self::En => "EN", Self::Fr => "FR" }
    }

    pub fn t(self) -> &'static T {
        match self { Self::En => &EN, Self::Fr => &FR }
    }
}

pub struct T {
    // Header
    pub subtitle: &'static str,
    pub mobile_filters: &'static str,
    pub mobile_close: &'static str,
    pub vdm_data_prefix: &'static str,
    pub telraam_data_prefix: &'static str,
    pub cdn_ndg_data_prefix: &'static str,
    pub last_api_fetch: &'static str,
    pub last_record:    &'static str,
    pub last_bike:      &'static str,
    pub last_successful_download: &'static str,
    pub last_bike_record: &'static str,
    pub received:     &'static str,
    pub data_through: &'static str,
    pub value_unavailable: &'static str,

    // Sidebar
    pub locations: &'static str,
    pub modalities: &'static str,
    pub resolution: &'static str,
    pub date_range: &'static str,
    pub custom_range: &'static str,
    pub year_on_year: &'static str,
    pub winter_on_winter: &'static str,
    pub range_too_short: &'static str,
    pub daily_averaging: &'static str,
    pub weekday: &'static str,
    pub weekend: &'static str,

    // Modalities
    pub bikes: &'static str,
    pub pedestrians: &'static str,
    pub cars: &'static str,
    pub trucks: &'static str,

    // Resolutions
    pub hour: &'static str,
    pub day: &'static str,
    pub week: &'static str,
    pub month: &'static str,

    // Date presets (some get formatted with a year)
    pub all_dates: &'static str,
    pub last_48h: &'static str,
    pub last_week: &'static str,
    pub last_month: &'static str,
    pub last_3_months: &'static str,
    pub last_6_months: &'static str,
    pub last_year: &'static str,
    pub summer: &'static str,
    pub winter: &'static str,

    // Map / chart placeholders
    pub loading_stations: &'static str,
    pub click_marker: &'static str,
    pub select_locations: &'static str,

    // Chart export
    pub download_chart_png: &'static str,
    pub copy_chart_to_clipboard: &'static str,
    pub copied_to_clipboard: &'static str,
}

pub const EN: T = T {
    subtitle: "Traffic Count Aggregator",
    mobile_filters: "Filters",
    mobile_close: "Close",
    vdm_data_prefix: "VdM data",
    telraam_data_prefix: "Telraam",
    cdn_ndg_data_prefix: "CDN-NDG",
    last_api_fetch: "Last API call",
    last_record:    "Last record received",
    last_bike:      "Last hour with bikes",
    last_successful_download: "Last successful download",
    last_bike_record: "Last record with bikes",
    received:     "Received",
    data_through: "Data through",
    value_unavailable: "—",

    locations: "Locations",
    modalities: "Modalities",
    resolution: "Counts per",
    date_range: "Date range",
    custom_range: "Custom range",
    year_on_year: "Year-on-Year",
    winter_on_winter: "Winter-on-Winter",
    range_too_short: "Range too short for resolution",
    daily_averaging: "Daily averaging",
    weekday: "Weekday",
    weekend: "Weekend",

    bikes: "Bikes",
    pedestrians: "Pedestrians",
    cars: "Cars",
    trucks: "Trucks",

    hour: "Hour",
    day: "Day",
    week: "Week",
    month: "Month",

    all_dates: "All dates",
    last_48h: "Last 48H",
    last_week: "Last Week",
    last_month: "Last Month",
    last_3_months: "Last 3 Months",
    last_6_months: "Last 6 Months",
    last_year: "Last Year",
    summer: "Summer",
    winter: "Winter",

    loading_stations: "Loading stations…",
    click_marker: "Click a marker to select / deselect",
    select_locations: "Please select one or more locations to view counts",

    download_chart_png: "Download chart as PNG",
    copy_chart_to_clipboard: "Copy chart to clipboard",
    copied_to_clipboard: "Copied!",
};

pub const FR: T = T {
    subtitle: "Agrégateur de comptages de circulation",
    mobile_filters: "Filtres",
    mobile_close: "Fermer",
    vdm_data_prefix: "Données VdM",
    telraam_data_prefix: "Telraam",
    cdn_ndg_data_prefix: "CDN-NDG",
    last_api_fetch: "Dernier appel API",
    last_record:    "Dernier relevé reçu",
    last_bike:      "Dernière heure avec vélos",
    last_successful_download: "Dernier téléchargement réussi",
    last_bike_record: "Dernier relevé avec vélos",
    received:     "Reçu",
    data_through: "Données jusqu'au",
    value_unavailable: "—",

    locations: "Emplacements",
    modalities: "Modalités",
    resolution: "Comptages par",
    date_range: "Plage de dates",
    custom_range: "Plage personnalisée",
    year_on_year: "Comparaison annuelle",
    winter_on_winter: "Comparaison hivernale",
    range_too_short: "Plage trop courte pour la résolution",
    daily_averaging: "Moyenne quotidienne",
    weekday: "Semaine",
    weekend: "Fin de semaine",

    bikes: "Vélos",
    pedestrians: "Piétons",
    cars: "Voitures",
    trucks: "Camions",

    hour: "Heure",
    day: "Jour",
    week: "Semaine",
    month: "Mois",

    all_dates: "Toutes les dates",
    last_48h: "Dernières 48 h",
    last_week: "Semaine dernière",
    last_month: "Mois dernier",
    last_3_months: "3 derniers mois",
    last_6_months: "6 derniers mois",
    last_year: "Année dernière",
    summer: "Été",
    winter: "Hiver",

    loading_stations: "Chargement des emplacements…",
    click_marker: "Cliquez sur un marqueur pour sélectionner / désélectionner",
    select_locations: "Veuillez sélectionner un ou plusieurs emplacements pour afficher les comptages",

    download_chart_png: "Télécharger le graphique en PNG",
    copy_chart_to_clipboard: "Copier le graphique dans le presse-papiers",
    copied_to_clipboard: "Copié !",
};
