//! Site climatology export: an area-sampled, identity-stamped summary of the
//! endpoint P4 monthly climate for one author-chosen region.
//!
//! Design truth: `docs/superpowers/specs/2026-10-26-site-climatology-export-design.md`.
//! The export is a read-only query over the published final state; it is not
//! a build-graph artifact and owns no cache entry.

use serde::Serialize;
use thiserror::Error;

use crate::world::natural::{ClimateModelProfile, NaturalQualityProfile};
use crate::world::CellId;

/// The only export schema written by this build.
pub const SITE_CLIMATOLOGY_SCHEMA_VERSION: u16 = 2;

/// Wet-day definition: daily precipitation above 0.1 mm.
///
/// New, Lister, Hulme & Makin (2002), *Climate Research* 21:1–25, §2.3: CRU
/// CL 2.0 wet-day frequencies are "the number of days per month with
/// precipitation >0.1 mm"; 1.0 mm station normals were converted to this
/// threshold. Downstream weather generators judge wet days against the same
/// value so their statistics stay comparable with any later wet-day source.
pub const WET_DAY_THRESHOLD_MM: f64 = 0.1;

/// Maximum Eady growth-rate coefficient, `sigma = 0.31 f |dU/dz| / N`.
///
/// Lindzen & Farrell (1980), *J. Atmos. Sci.* 37(7); used as a storm-track
/// diagnostic by Hoskins & Valdes (1990), *J. Atmos. Sci.* 47(15).
pub const EADY_GROWTH_RATE_COEFFICIENT: f64 = 0.31;

/// Minimum anchor distance from the coast, in P4 climate-grid spacings, for a
/// recommended inland site.
///
/// User decision recorded in Voxim `Docs/Weather.md` §7 item 4 (2026-10-26):
/// two spacings keep the expanded sampling footprint from averaging ocean
/// cells of the coarse climate grid into an inland site.
pub const ANCHOR_INLAND_MIN_CLIMATE_SPACINGS: f64 = 2.0;

/// Number of anchor candidates offered by one recommendation query.
pub const ANCHOR_RECOMMENDATION_COUNT: usize = 5;

/// Authoring prior for the current Qinglan consumer, not a physical support scale.
/// Source: Voxim `Docs/R6.md`, `world_half_extent_m`.
pub(crate) const QINGLAN_HALF_EXTENT_M: f64 = 8_192.0;

/// Author-chosen mapping from a flat game world onto the planet.
///
/// The anchor is the centroid of an authoritative surface cell. The heading
/// `rho` rotates the game's canonical +X axis clockwise (seen from above) away
/// from geographic east, so `rho = 0` puts +X east and +Z south, matching
/// Voxim's Y-up canonical frame where X→Z is clockwise from above.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SiteRegion {
    anchor_cell: CellId,
    heading_deg: f64,
    scale: f64,
    half_extent_m: f64,
}

impl SiteRegion {
    /// Validates a region mapping.
    pub fn new(
        anchor_cell: CellId,
        heading_deg: f64,
        scale: f64,
        half_extent_m: f64,
    ) -> Result<Self, SiteClimatologyError> {
        if !heading_deg.is_finite() {
            return Err(SiteClimatologyError::InvalidRegion("heading_deg"));
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err(SiteClimatologyError::InvalidRegion("scale"));
        }
        if !half_extent_m.is_finite() || half_extent_m <= 0.0 {
            return Err(SiteClimatologyError::InvalidRegion("half_extent_m"));
        }
        Ok(Self {
            anchor_cell,
            heading_deg,
            scale,
            half_extent_m,
        })
    }

    /// Returns the anchor cell.
    pub const fn anchor_cell(&self) -> CellId {
        self.anchor_cell
    }

    /// Returns the clockwise rotation from geographic east to game +X.
    pub const fn heading_deg(&self) -> f64 {
        self.heading_deg
    }

    /// Returns planet metres per game metre.
    pub const fn scale(&self) -> f64 {
        self.scale
    }

    /// Returns the game world's half side length in game metres.
    pub const fn half_extent_m(&self) -> f64 {
        self.half_extent_m
    }

    /// Returns the same mapping anchored at another cell.
    pub const fn with_anchor(self, anchor_cell: CellId) -> Self {
        Self {
            anchor_cell,
            ..self
        }
    }
}

/// Terrain source of a site query; current climate is independent of author history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteTerrainSource {
    /// Sekai formation product.
    Formation,
    /// Fixed-scale Civ Atlas terrain with current Sekai climate.
    CivAtlas,
}

/// Identity of the published world the export was sampled from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteClimatologySource {
    /// Actual terrain source, without implying a formation history.
    pub terrain_source: SiteTerrainSource,
    /// Fixed Atlas author scale; independent of climate and display resolution.
    pub fixed_terrain_scale: Option<u8>,
    /// Root seed of the build.
    pub world_seed: u64,
    /// `SurfaceRef` fingerprint of the authoritative surface, hex.
    pub surface_fingerprint: String,
    /// Engine-audited result hash of the whole build, hex.
    pub build_result_hash: String,
    /// Presentation graph contract version for the recorded terrain source.
    pub graph_contract_version: u16,
    /// Canonical P4 field-state fingerprint (`GlobalCirculationFields::fingerprint`), hex.
    pub global_circulation_fingerprint: String,
    /// Published `GlobalCirculationSnapshot` schema version.
    pub global_circulation_schema: u16,
    /// Closed P4 model profile.
    pub climate_profile: ClimateModelProfile,
    /// Actual C2 work-domain quality profile; not the terrain cell count.
    pub quality_profile: NaturalQualityProfile,
    /// Cargo package version of the exporting build.
    pub sekai_package_version: String,
}

/// Sampled region geometry and surface statistics.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteRegionSummary {
    /// Anchor cell id.
    pub anchor_cell: u32,
    /// Anchor latitude, degrees north.
    pub anchor_lat_deg: f64,
    /// Anchor longitude, degrees east.
    pub anchor_lon_deg: f64,
    /// Clockwise rotation from geographic east to game +X, degrees.
    pub heading_deg: f64,
    /// Planet metres per game metre.
    pub scale: f64,
    /// Game world half side, game metres.
    pub half_extent_m: f64,
    /// Requested footprint side on the planet, metres.
    pub requested_footprint_side_m: f64,
    /// Mean P4 climate work-grid spacing, metres.
    pub climate_grid_spacing_m: f64,
    /// Footprint side actually sampled, metres.
    pub effective_footprint_side_m: f64,
    /// Whether the footprint was widened to the climate grid spacing.
    pub footprint_expanded: bool,
    /// Authoritative cells whose centroid lies in the effective footprint.
    pub cell_count: u32,
    /// Area-weighted fraction of source cells classified as dry land.
    pub land_fraction: f64,
    /// Area-weighted fraction of source cells classified as lakes.
    pub lake_fraction: f64,
    /// Elevation statistics of sampled cells.
    pub elevation_m: SiteElevationSummary,
}

/// Area-weighted mean and range of current surface elevation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SiteElevationSummary {
    pub mean: f64,
    pub min: f64,
    pub max: f64,
}

/// Recorded evaluation of the anchor against the inland-site criteria.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnchorAssessment {
    /// Great-circle distance from the anchor to the nearest coastal ocean
    /// cell centroid, metres; null on a planet without ocean.
    pub coast_distance_m: Option<f64>,
    /// Inland threshold, `ANCHOR_INLAND_MIN_CLIMATE_SPACINGS` climate spacings, metres.
    pub inland_threshold_m: f64,
    /// Whether the anchor cell is dry land.
    pub anchor_is_land: bool,
    /// Whether the coast distance meets the inland threshold.
    pub inland: bool,
    /// Whether every sampled cell is dry land (no ocean, no lake).
    pub footprint_dry_land_only: bool,
    /// Number of published lake cells in the footprint.
    pub footprint_lake_cells: u32,
    /// Elevation range across the footprint, metres; recommendations rank on it.
    pub footprint_relief_m: f64,
    /// Whether every criterion holds.
    pub meets_criteria: bool,
}

/// Explicit availability of one exported quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteAvailability {
    Available,
    EvaluatedNotApplicable,
    Unavailable,
}

/// Exported quantities whose absence must never be inferred from a null.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteCapabilityId {
    SteeringWind,
    VerticalWindShear,
    EadyGrowthRate,
    RelativeHumidity,
    WetDayStatistics,
    ConvectivePrecipitationShare,
    DiurnalTemperatureRange,
    WindVariability,
    CloudCover,
    TropicalCycloneClimatology,
}

/// Stable machine reasons for a non-available capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteCapabilityReason {
    /// P4 published no upper layer (C1 single-layer profile).
    NoUpperLayer,
    /// No verified relationship from monthly precipitation to wet-day frequency.
    NoVerifiedWetDayRelationship,
    /// P4 does not separate convective from large-scale precipitation.
    NoPrecipitationTypeDiagnostic,
    /// P4 publishes monthly means only, without a diurnal cycle.
    NoDiurnalCycle,
    /// P4 publishes monthly mean vectors only, without transient variance.
    NoTransientStatistics,
    /// The cloud and surface-energy stage (P6) is not implemented.
    CloudsNotImplemented,
    /// Recorded verbatim from the P4 capability inventory.
    P4CapabilityUnavailable,
}

/// Substitute source used in place of an unavailable capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteSubstitute {
    NearSurfaceWind,
}

/// One capability entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SiteCapabilityStatus {
    pub id: SiteCapabilityId,
    pub availability: SiteAvailability,
    pub reason: Option<SiteCapabilityReason>,
    pub substitute: Option<SiteSubstitute>,
}

/// One month of sampled and derived climatology.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteClimatologyMonth {
    /// Calendar month, 1–12.
    pub month: u8,
    pub air_temperature_c: f64,
    pub precipitation_mm_day: f64,
    pub orographic_precipitation_mm_day: f64,
    pub specific_humidity_kg_kg: f64,
    pub relative_humidity: f64,
    /// Near-surface wind `[east, north]` at the anchor, m/s.
    pub near_surface_wind_en_m_s: [f64; 2],
    /// Upper-layer (steering) wind `[east, north]`, m/s; null when unavailable.
    pub steering_wind_en_m_s: Option<[f64; 2]>,
    /// Upper minus lower wind `[east, north]`, m/s; null when unavailable.
    pub wind_shear_en_m_s: Option<[f64; 2]>,
    /// Geographic bearing the steering flow blows toward (north = 0, clockwise), degrees.
    /// Uses the near-surface wind when steering wind is unavailable.
    pub steering_flow_toward_bearing_deg: f64,
    /// Maximum Eady growth rate, per day; null when unavailable.
    pub eady_growth_rate_per_day: Option<f64>,
    /// Wet-day statistics; null while `wet_day_statistics` is unavailable.
    pub wet_day_fraction: Option<f64>,
    pub p_wet_given_dry: Option<f64>,
    pub p_wet_given_wet: Option<f64>,
    pub mean_wet_day_amount_mm: Option<f64>,
}

/// Method identifiers for every derived quantity, keyed by field.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteMethods {
    pub aggregation: &'static str,
    pub wind_projection: &'static str,
    pub relative_humidity: &'static str,
    pub steering_flow_bearing: &'static str,
    pub eady_growth_rate: &'static str,
    pub wet_day_threshold: &'static str,
}

/// Complete export document. Field order is the serialized key order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteClimatology {
    pub schema_version: u16,
    pub source: SiteClimatologySource,
    pub region: SiteRegionSummary,
    pub anchor_assessment: AnchorAssessment,
    pub wet_day_threshold_mm: f64,
    pub capabilities: Vec<SiteCapabilityStatus>,
    pub months: Vec<SiteClimatologyMonth>,
    pub methods: SiteMethods,
}

impl SiteClimatology {
    /// Serializes the export deterministically (fixed key order, serde_json
    /// default float formatting).
    pub fn to_json_bytes(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).expect("site climatology contains only finite values")
    }

    /// Returns the availability recorded for one capability.
    pub fn availability(&self, id: SiteCapabilityId) -> SiteAvailability {
        self.capabilities
            .iter()
            .find(|status| status.id == id)
            .map(|status| status.availability)
            .expect("every export lists every capability")
    }
}

/// Export fingerprint: plain BLAKE3 of the exported bytes, so `b3sum` on the
/// written file reproduces it.
pub fn site_climatology_fingerprint(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}

/// Failures of a site climatology query.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SiteClimatologyError {
    /// A region parameter is non-finite or non-positive.
    #[error("invalid site region parameter `{0}`")]
    InvalidRegion(&'static str),
    /// The anchor cell does not exist on the authoritative surface.
    #[error("anchor cell is not on the authoritative surface")]
    UnknownAnchor,
    /// No cell centroid lies inside the footprint.
    #[error("site footprint contains no surface cell")]
    EmptyFootprint,
    /// Input fields do not share the authoritative cell count.
    #[error("input field `{field}` has {found} cells; surface has {expected}")]
    CellCountMismatch {
        field: &'static str,
        found: usize,
        expected: usize,
    },
}
