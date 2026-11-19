//! Fixed author bed, upstream Civ Atlas water classes and current Sekai climate/runoff,
//! with marine inundation resolved on the author bed (`2026-11-03-world-truth-scope-design.md` R3).

pub(crate) mod coast;

use std::sync::Arc;

use civ_atlas_terrain::{FineTerrain, LocatedSurfaceSample, TerrainError};
use serde::Serialize;
use thiserror::Error;

use super::{
    GlobalCirculationSnapshot, LandOceanField, NaturalQualityReport, SurfaceWaterField,
    SurfaceWaterKind, CLIMATE_MONTH_COUNT, WATER_VOLUME_RELATIVE_TOLERANCE,
};
use crate::world::spatial::{dot, SphericalSurfaceSnapshot, SurfaceRef, UnitVector3};
use crate::world::CellId;

/// Identity version for fixed-bed water classes, material prior and current runoff.
/// Version 5 resolves marine inundation from submerged pathways to source ocean sites.
pub const ATLAS_TERRAIN_MODEL_VERSION: u32 = 5;
/// Atomic Atlas product epoch, including its climate-derived potential vegetation.
pub const ATLAS_CURRENT_VERSION: u32 = 8;
/// Fixed original zoom scale; camera LOD never changes the author bed.
pub const ATLAS_FINE_SCALE: civ_atlas_terrain::FineScale = civ_atlas_terrain::FineScale::Four;
/// Fan quadrature level of the cell water fields: each source fan triangle is
/// split `level - 1` times into equal children and sampled at leaf centres.
/// Level 2 (about 24 points per cell) was chosen from the measured noise floor
/// against the old canonical support, 2.4% mean absolute ocean-fraction error
/// on mixed cells, against 6.9% at level 1 and 0.85% at level 3 at about 3.9x
/// the evaluation cost (`measurements/2026-11-03-truth-layer-cell-quadrature.md` §2-§3).
pub const ATLAS_SUPPORT_DEPTH: u8 = 2;
/// One dyadic parent of the original atlas raster is the initial coastal search scale.
/// The R3 comparison measured the one-pixel and two-pixel starts, then subdivided
/// only where actual submerged paths did not resolve the local topology.
pub(crate) const ATLAS_COAST_PARENT_RASTER_INTERVALS: u8 = 2;
/// Subdivisions of the source angular pixel used only to verify possible coastal basin divides.
/// The recorded 55.5 km regression uses this spacing. A native-pixel-only flood left
/// 790 further wet cut edges at this verification scale on the same world (R3 measurements).
pub(crate) const ATLAS_COAST_CLEARANCE_SUBDIVISIONS: u16 = 16;

/// Integrated fields on original source cells. Depth is volume / whole cell area,
/// matching the existing C2 consumer; lake area and volume are never ocean area.
#[derive(Debug, Clone, Serialize)]
pub struct AtlasCellFields {
    pub(crate) land_ocean: LandOceanField,
    pub(crate) bed_elevation_m: Vec<f32>,
    pub(crate) exposed_elevation_m: Vec<f32>,
    pub(crate) ocean_fraction: Vec<f32>,
    pub(crate) lake_fraction: Vec<f32>,
    pub(crate) dry_fraction: Vec<f32>,
    pub(crate) ocean_volume_m3: Vec<f64>,
    pub(crate) lake_volume_m3: Vec<f64>,
    pub(crate) relative_permeability: Vec<f32>,
}
impl AtlasCellFields {
    fn validate(&self, count: usize) -> Result<(), AtlasTerrainValidationError> {
        if self.land_ocean.len() != count {
            return Err(AtlasTerrainValidationError::SourceMismatch);
        }
        for values in [
            &self.bed_elevation_m,
            &self.exposed_elevation_m,
            &self.ocean_fraction,
            &self.lake_fraction,
            &self.dry_fraction,
            &self.relative_permeability,
        ] {
            if values.len() != count || values.iter().any(|v| !v.is_finite()) {
                return Err(AtlasTerrainValidationError::InvalidPayload);
            }
        }
        for values in [
            &self.ocean_fraction,
            &self.lake_fraction,
            &self.dry_fraction,
            &self.relative_permeability,
        ] {
            if values.iter().any(|v| !(0.0..=1.0).contains(v)) {
                return Err(AtlasTerrainValidationError::InvalidPayload);
            }
        }
        for values in [&self.ocean_volume_m3, &self.lake_volume_m3] {
            if values.len() != count || values.iter().any(|v| !v.is_finite() || *v < 0.0) {
                return Err(AtlasTerrainValidationError::InvalidPayload);
            }
        }
        Ok(())
    }
    /// Source-centre marine connectivity, independent of cell mean bed or wet fraction.
    pub const fn land_ocean(&self) -> &LandOceanField {
        &self.land_ocean
    }
    /// Area mean of the sampled fixed bed, in metres.
    pub fn bed_elevation_m(&self) -> &[f32] {
        &self.bed_elevation_m
    }
    /// Mean exposed surface, including the initialized constant water surfaces.
    pub fn exposed_elevation_m(&self) -> &[f32] {
        &self.exposed_elevation_m
    }
    /// Connected sea fraction of each whole source cell.
    pub fn ocean_fraction(&self) -> &[f32] {
        &self.ocean_fraction
    }
    /// Initialized inland fresh-water fraction, separate from sea.
    pub fn lake_fraction(&self) -> &[f32] {
        &self.lake_fraction
    }
    /// Actual dry area fraction used by both evapotranspiration and runoff.
    pub fn dry_fraction(&self) -> &[f32] {
        &self.dry_fraction
    }
    /// Integrated ocean stock per cell, in cubic metres.
    pub fn ocean_volume_m3(&self) -> &[f64] {
        &self.ocean_volume_m3
    }
    /// Integrated initial lake stock per cell, in cubic metres.
    pub fn lake_volume_m3(&self) -> &[f64] {
        &self.lake_volume_m3
    }
    /// A4 basement endmember prior; not measured absolute permeability.
    pub fn relative_permeability(&self) -> &[f32] {
        &self.relative_permeability
    }
}

/// Water body of one source site, taken from the upstream coarse classes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) enum AtlasSiteBody {
    /// Upstream ocean site; sea level is the reference zero.
    Ocean,
    /// Upstream lake site of connected lake `lake` with its constant surface.
    Lake { lake: usize, surface_m: f64 },
}

/// Source-site bodies and the marine footprint shared by all subcell consumers.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AtlasSiteWater {
    /// One entry per source site; `None` is dry land.
    pub sites: Vec<Option<AtlasSiteBody>>,
    coast: coast::CoastMap,
}
impl AtlasSiteWater {
    pub(crate) fn new(sites: Vec<Option<AtlasSiteBody>>, coast: coast::CoastMap) -> Self {
        Self { sites, coast }
    }

    /// A negative bed is marine only in a component reached by a submerged ocean source.
    pub(crate) fn is_ocean(&self, sample: &LocatedSurfaceSample) -> bool {
        sample.bed_elevation_m < 0.0 && self.coast.is_marine(sample.source_face, sample.weights)
    }

    fn source_field(&self) -> SurfaceWaterField {
        SurfaceWaterField::from_kinds(
            self.sites
                .iter()
                .map(|body| match body {
                    Some(AtlasSiteBody::Ocean) => SurfaceWaterKind::Ocean,
                    Some(AtlasSiteBody::Lake { .. }) => SurfaceWaterKind::Lake,
                    None => SurfaceWaterKind::DryLand,
                })
                .collect(),
        )
    }

    /// Shared point classification for inventory, picking and display. Lake levels are
    /// the original source priors and remain separate from marine inundation.
    pub(crate) fn classify(
        &self,
        corners: [usize; 3],
        sample: &LocatedSurfaceSample,
    ) -> (SurfaceWaterKind, Option<f64>, Option<usize>) {
        if self.is_ocean(sample) {
            return (SurfaceWaterKind::Ocean, Some(0.0), None);
        }
        let bodies = corners.map(|i| self.sites[i]);
        let lake = bodies
            .iter()
            .filter_map(|body| match body {
                Some(AtlasSiteBody::Lake { lake, surface_m }) => Some((*lake, *surface_m)),
                _ => None,
            })
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
        match lake {
            Some((lake, surface)) if sample.bed_elevation_m < surface => {
                (SurfaceWaterKind::Lake, Some(surface), Some(lake))
            }
            _ => (SurfaceWaterKind::DryLand, None, None),
        }
    }
}

/// An upstream lake on the fixed bed; evaporation is not modeled by C2.
#[derive(Debug, Clone, Serialize)]
pub struct AtlasLake {
    pub(crate) surface_elevation_m: f64,
    pub(crate) area_m2: f64,
    pub(crate) volume_m3: f64,
    pub(crate) resolved_monthly_inflow_m3_s: [f64; CLIMATE_MONTH_COUNT],
    pub(crate) resolved_monthly_outflow_m3_s: [f64; CLIMATE_MONTH_COUNT],
    pub(crate) direct_monthly_precipitation_m3_s: [f64; CLIMATE_MONTH_COUNT],
}
impl AtlasLake {
    /// Constant initialized lake surface, in metres.
    pub const fn surface_elevation_m(&self) -> f64 {
        self.surface_elevation_m
    }
    /// Lake area under the R1 point rule and fixed fan quadrature, in square metres;
    /// zero (with zero volume) for a lake smaller than the quadrature resolution.
    pub const fn area_m2(&self) -> f64 {
        self.area_m2
    }
    /// Current initial volume; no time advance is implied.
    pub const fn volume_m3(&self) -> f64 {
        self.volume_m3
    }
    /// Current resolved river inflow from C2 precipitation and dry-land runoff.
    pub fn monthly_inflow_m3_s(&self) -> &[f64; CLIMATE_MONTH_COUNT] {
        &self.resolved_monthly_inflow_m3_s
    }
    /// Resolved outflow to a source-grid outlet, zero while the lake retains inflow.
    pub fn monthly_outflow_m3_s(&self) -> &[f64; CLIMATE_MONTH_COUNT] {
        &self.resolved_monthly_outflow_m3_s
    }
    /// Direct precipitation over the initialized lake area, independent of dry runoff.
    pub fn monthly_precipitation_m3_s(&self) -> &[f64; CLIMATE_MONTH_COUNT] {
        &self.direct_monthly_precipitation_m3_s
    }
    /// Net resolved input, not total dV/dt: lake evaporation and groundwater are unavailable.
    pub fn monthly_resolved_net_input_m3_s(&self) -> [f64; CLIMATE_MONTH_COUNT] {
        std::array::from_fn(|m| {
            self.resolved_monthly_inflow_m3_s[m] + self.direct_monthly_precipitation_m3_s[m]
                - self.resolved_monthly_outflow_m3_s[m]
        })
    }
}

/// Current resolved channel between two source sites, driven by Sekai precipitation.
#[derive(Debug, Clone, Serialize)]
pub struct AtlasRiverSegment {
    /// Unit directions of the segment ends.
    pub endpoints: [[f64; 3]; 2],
    /// Mean of the current twelve monthly discharges, in cubic metres per second.
    pub mean_discharge_m3_s: f64,
}

/// Resolved routing budget. Unmodeled lake evaporation is deliberately absent.
#[derive(Debug, Clone, Serialize)]
pub struct AtlasRunoffBudget {
    /// Generated dry-land runoff before routing, in cubic metres per second.
    pub monthly_runoff_m3_s: [f64; CLIMATE_MONTH_COUNT],
    /// Direct rain on lake surfaces, not counted as dry-land runoff.
    pub monthly_direct_lake_precipitation_m3_s: [f64; CLIMATE_MONTH_COUNT],
    /// Runoff entering ocean terminals, including dry fractions in coastal ocean cells.
    pub monthly_ocean_delivery_m3_s: [f64; CLIMATE_MONTH_COUNT],
    /// Runoff retained at initialized sub-spill lake terminals.
    pub monthly_lake_retention_m3_s: [f64; CLIMATE_MONTH_COUNT],
    /// Current runoff collecting at initially dry, unseeded depressions.
    pub monthly_dry_depression_retention_m3_s: [f64; CLIMATE_MONTH_COUNT],
    /// Numerical residual in the declared routing balance, without missing processes.
    pub monthly_routing_residual_m3_s: [f64; CLIMATE_MONTH_COUNT],
}

impl AtlasRunoffBudget {
    /// The two source and three terminal rates (m³/s) shown in the monthly water budget.
    pub(crate) fn monthly_terms_m3_s(&self, month: usize) -> [f64; 5] {
        [
            self.monthly_runoff_m3_s[month],
            self.monthly_direct_lake_precipitation_m3_s[month],
            self.monthly_ocean_delivery_m3_s[month],
            self.monthly_lake_retention_m3_s[month],
            self.monthly_dry_depression_retention_m3_s[month],
        ]
    }

    /// Recompute sources minus terminals in m³/s for a zero-based model month.
    pub(crate) fn routing_residual_m3_s(&self, month: usize) -> f64 {
        let [runoff, rain, ocean, lake, dry] = self.monthly_terms_m3_s(month);
        runoff + rain - ocean - lake - dry
    }

    fn validate(&self, lakes: &[AtlasLake]) -> Result<(), AtlasTerrainValidationError> {
        let valid_rate = |rate: f64| rate.is_finite() && rate >= 0.0;
        let mut lake_rain = [0.0; CLIMATE_MONTH_COUNT];
        let mut lake_retention = [0.0; CLIMATE_MONTH_COUNT];
        for lake in lakes {
            for (m, net) in lake
                .monthly_resolved_net_input_m3_s()
                .into_iter()
                .enumerate()
            {
                let rain = lake.direct_monthly_precipitation_m3_s[m];
                let inflow = lake.resolved_monthly_inflow_m3_s[m];
                let outflow = lake.resolved_monthly_outflow_m3_s[m];
                if ![inflow, rain, outflow].into_iter().all(valid_rate)
                    || !net.is_finite()
                    || net < -(inflow + rain) * WATER_VOLUME_RELATIVE_TOLERANCE
                {
                    return Err(AtlasTerrainValidationError::RunoffMismatch);
                }
                lake_rain[m] += rain;
                lake_retention[m] += net;
            }
        }
        for m in 0..CLIMATE_MONTH_COUNT {
            let terms = self.monthly_terms_m3_s(m);
            let [runoff, rain, _, retained, _] = terms;
            let input = runoff + rain;
            let tolerance = input * WATER_VOLUME_RELATIVE_TOLERANCE;
            let residual = self.routing_residual_m3_s(m);
            if !terms.into_iter().all(valid_rate)
                || !input.is_finite()
                || !residual.is_finite()
                || self.monthly_routing_residual_m3_s[m] != residual
                || residual.abs() > tolerance
                || !lake_retention[m].is_finite()
                || (lake_rain[m] - rain).abs() > tolerance
                || (lake_retention[m] - retained).abs() > tolerance
            {
                return Err(AtlasTerrainValidationError::RunoffMismatch);
            }
        }
        Ok(())
    }
}

/// A point query from the immutable bed, connected ocean and source lake levels.
#[derive(Debug, Clone, Copy)]
pub struct AtlasSurfaceSample {
    /// Exact canonical fine bed, in metres.
    pub bed_elevation_m: f64,
    /// Current surface water class; unseeded inland depressions are dry.
    pub water: SurfaceWaterKind,
    /// Constant water surface where wet, otherwise unavailable.
    pub water_surface_elevation_m: Option<f64>,
    /// Original source-cell identity for projected climate and material fields.
    pub source_cell: usize,
}
impl AtlasSurfaceSample {
    /// Positive current water depth in metres; absent on dry land.
    pub fn water_depth_m(self) -> Option<f64> {
        self.water_surface_elevation_m
            .map(|surface| surface - self.bed_elevation_m)
    }
}

/// Runtime-only current-state product. Serialization records identity and public
/// fields, never a loadable cache of the private author generator or solver history.
#[derive(Debug, Serialize)]
pub struct AtlasTerrainSnapshot {
    pub(crate) fingerprint: [u8; 32],
    pub(crate) terrain_fingerprint: [u8; 32],
    pub(crate) forcing_fingerprint: [u8; 32],
    pub(crate) surface_ref: SurfaceRef,
    pub(crate) support_depth: u8,
    pub(crate) initial_water_inventory_m3: f64,
    pub(crate) fields: AtlasCellFields,
    pub(crate) climate: GlobalCirculationSnapshot,
    pub(crate) climate_quality: NaturalQualityReport,
    pub(crate) vegetation: super::biome::PotentialVegetation,
    pub(crate) lakes: Vec<AtlasLake>,
    pub(crate) rivers: Vec<AtlasRiverSegment>,
    pub(crate) runoff_budget: AtlasRunoffBudget,
    #[serde(skip)]
    pub(crate) surface: Arc<SphericalSurfaceSnapshot>,
    #[serde(skip)]
    pub(crate) fine: Arc<FineTerrain>,
    #[serde(skip)]
    pub(crate) site_water: AtlasSiteWater,
}
impl AtlasTerrainSnapshot {
    /// Potential vegetation category for the dry portion of each source cell.
    pub fn potential_biomes(&self) -> &[u32] {
        &self.vegetation.categories
    }
    /// Whether the source climate lies outside the digitized biome diagram.
    pub fn extrapolated_biomes(&self) -> &[bool] {
        &self.vegetation.extrapolated
    }
    /// Original source cells with validated spherical geometry.
    pub fn surface(&self) -> &SphericalSurfaceSnapshot {
        &self.surface
    }
    /// Immutable canonical source evaluator; its scale cannot follow the camera.
    pub fn fine_terrain(&self) -> &FineTerrain {
        &self.fine
    }
    /// Current content identity, including source, water, C2 and runoff.
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
    /// Geometry/material/water identity supplied to C2.
    pub const fn terrain_fingerprint(&self) -> &[u8; 32] {
        &self.terrain_fingerprint
    }
    /// Integrated source-cell fields.
    pub const fn fields(&self) -> &AtlasCellFields {
        &self.fields
    }
    /// Build the source-site water classes for source-scale fields and regional sampling.
    pub(crate) fn source_water_field(&self) -> SurfaceWaterField {
        self.site_water.source_field()
    }
    /// Actual current Sekai C2 output.
    pub const fn climate(&self) -> &GlobalCirculationSnapshot {
        &self.climate
    }
    /// Existing P4 suggestions and hard-closure evidence for this exact current state.
    pub const fn climate_quality(&self) -> &NaturalQualityReport {
        &self.climate_quality
    }
    /// Current initialized lakes and their resolved rates.
    pub fn lakes(&self) -> &[AtlasLake] {
        &self.lakes
    }
    /// Current channels computed from Sekai precipitation.
    pub fn rivers(&self) -> &[AtlasRiverSegment] {
        &self.rivers
    }
    /// Declared runoff conservation evidence.
    pub const fn runoff_budget(&self) -> &AtlasRunoffBudget {
        &self.runoff_budget
    }
    /// Initial stock integrated from the same declared geometry.
    pub const fn initial_water_inventory_m3(&self) -> f64 {
        self.initial_water_inventory_m3
    }
    /// Fixed fan quadrature level of the cell water fields.
    pub const fn support_depth(&self) -> u8 {
        self.support_depth
    }

    /// Sample the fixed bed and initialized connected water; rejects invalid directions.
    pub fn sample_surface(&self, direction: [f64; 3]) -> Result<AtlasSurfaceSample, TerrainError> {
        self.bind_surface_sample(direction, &self.fine.sample_located_surface(direction)?)
    }

    /// Sample a caller batch with shared source scratch. Preserves order and
    /// returns no partial result on invalid input or cooperative cancellation.
    pub fn sample_surface_batch(
        &self,
        directions: &[[f64; 3]],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<AtlasSurfaceSample>, TerrainError> {
        let beds = self
            .fine
            .sample_located_surface_batch(directions, cancelled)?;
        directions
            .iter()
            .zip(beds)
            .map(|(&direction, sample)| {
                if cancelled() {
                    return Err(TerrainError::Cancelled);
                }
                self.bind_surface_sample(direction, &sample)
            })
            .collect()
    }

    /// Sample the center of a display leaf in its known source-cell fan.
    /// `cell` supplies the published field identity. The bed and water use the point
    /// sample in `direction`, with the same coverage errors as [`Self::sample_surface`].
    pub(crate) fn sample_surface_in_cell(
        &self,
        cell: CellId,
        direction: UnitVector3,
    ) -> Result<AtlasSurfaceSample, TerrainError> {
        let sample = self.fine.sample_located_surface(direction.components())?;
        let corners = source_corners(&self.fine, sample.source_face);
        Ok(self.classify_surface_sample(&sample, corners, cell.raw() as usize))
    }

    /// Evaluate leaf centers within one source-cell fan, in input order with shared bed scratch.
    /// Returns the bed sampling error before yielding any readings if coverage or input fails.
    pub(crate) fn sample_surface_in_cell_batch(
        &self,
        cell: CellId,
        directions: &[[f64; 3]],
    ) -> Result<impl Iterator<Item = AtlasSurfaceSample> + '_, TerrainError> {
        let beds = self
            .fine
            .sample_located_surface_batch(directions, &|| false)?;
        Ok(beds.into_iter().map(move |sample| {
            let corners = source_corners(&self.fine, sample.source_face);
            self.classify_surface_sample(&sample, corners, cell.raw() as usize)
        }))
    }

    fn bind_surface_sample(
        &self,
        direction: [f64; 3],
        sample: &LocatedSurfaceSample,
    ) -> Result<AtlasSurfaceSample, TerrainError> {
        let corners = source_corners(&self.fine, sample.source_face);
        let owner = source_owner(&self.fine, &self.surface, corners, direction)?;
        Ok(self.classify_surface_sample(sample, corners, owner))
    }

    fn classify_surface_sample(
        &self,
        sample: &LocatedSurfaceSample,
        corners: [usize; 3],
        source_cell: usize,
    ) -> AtlasSurfaceSample {
        let (water, level, _) = self.site_water.classify(corners, sample);
        AtlasSurfaceSample {
            bed_elevation_m: sample.bed_elevation_m,
            water,
            water_surface_elevation_m: level,
            source_cell,
        }
    }

    /// Check source binding, actual current stock and resolved flow conservation.
    /// Returns a typed error for any mismatched or non-finite current state.
    pub fn validate(&self) -> Result<(), AtlasTerrainValidationError> {
        self.validate_binding()?;
        let n = self.surface.cells().len();
        self.fields.validate(n)?;
        if self.vegetation.categories.len() != n
            || self.vegetation.extrapolated.len() != n
            || self.fine.coarse_terrain().site_xyz().len() != 3 * n
            || self.site_water.sites.len() != n
            || self.site_water.coast.face_count()
                != self.fine.coarse_terrain().site_triangles().len() / 3
        {
            return Err(AtlasTerrainValidationError::SourceMismatch);
        }
        for (xyz, cell) in self
            .fine
            .coarse_terrain()
            .site_xyz()
            .chunks_exact(3)
            .zip(self.surface.cells())
        {
            let direction =
                UnitVector3::new(f64::from(xyz[0]), f64::from(xyz[1]), f64::from(xyz[2]))
                    .map_err(|_| AtlasTerrainValidationError::SourceMismatch)?;
            if direction != cell.site {
                return Err(AtlasTerrainValidationError::SourceMismatch);
            }
        }
        if self
            .site_water
            .sites
            .iter()
            .flatten()
            .any(|body| match *body {
                AtlasSiteBody::Ocean => false,
                AtlasSiteBody::Lake { lake, surface_m } => {
                    lake >= self.lakes.len() || surface_m != self.lakes[lake].surface_elevation_m
                }
            })
        {
            return Err(AtlasTerrainValidationError::InvalidPayload);
        }
        if self.terrain_fingerprint
            != terrain_identity(
                &self.fine,
                &self.surface,
                &self.site_water,
                &self.fields,
                self.support_depth,
            )
        {
            return Err(AtlasTerrainValidationError::SourceMismatch);
        }
        self.climate
            .validate_against(&self.surface)
            .map_err(|e| AtlasTerrainValidationError::Climate(e.to_string()))?;
        self.climate_quality
            .validate()
            .map_err(|e| AtlasTerrainValidationError::Climate(e.to_string()))?;
        let lake_stock = self.lakes.iter().map(|lake| lake.volume_m3).sum::<f64>();
        // A lake smaller than the declared quadrature has zero area and volume
        // together; it remains an upstream lake site and routing terminal.
        if self.lakes.iter().any(|lake| {
            !lake.volume_m3.is_finite()
                || lake.volume_m3 < 0.0
                || !lake.area_m2.is_finite()
                || lake.area_m2 < 0.0
                || (lake.area_m2 == 0.0) != (lake.volume_m3 == 0.0)
                || !lake.surface_elevation_m.is_finite()
        }) || super::water_volume_relative_error(
            lake_stock,
            self.fields.lake_volume_m3.iter().sum(),
        ) > WATER_VOLUME_RELATIVE_TOLERANCE
        {
            return Err(AtlasTerrainValidationError::WaterInventoryMismatch);
        }
        let realized = self
            .fields
            .ocean_volume_m3
            .iter()
            .chain(&self.fields.lake_volume_m3)
            .sum::<f64>();
        if !realized.is_finite()
            || !self.initial_water_inventory_m3.is_finite()
            || super::water_volume_relative_error(realized, self.initial_water_inventory_m3)
                > WATER_VOLUME_RELATIVE_TOLERANCE
        {
            return Err(AtlasTerrainValidationError::WaterInventoryMismatch);
        }
        self.runoff_budget.validate(&self.lakes)?;
        if self.fingerprint != self.calculate_fingerprint() {
            return Err(AtlasTerrainValidationError::SourceMismatch);
        }
        Ok(())
    }

    pub(crate) fn validate_binding(&self) -> Result<(), AtlasTerrainValidationError> {
        if self.surface_ref != SurfaceRef::for_spherical(&self.surface)
            || self.climate.surface_ref() != self.surface_ref
            || self.climate.checkpoint().forcing_fingerprint() != &self.forcing_fingerprint
            || self.climate_quality.surface_ref() != self.surface_ref
            || self.climate_quality.subject_fingerprint()
                != Some(self.climate.checkpoint().fingerprint())
            || self.fingerprint == [0; 32]
            || self.terrain_fingerprint == [0; 32]
        {
            return Err(AtlasTerrainValidationError::SourceMismatch);
        }
        Ok(())
    }

    pub(crate) fn calculate_fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.atlas-current.v1\0");
        hasher.update(&ATLAS_CURRENT_VERSION.to_le_bytes());
        hasher.update(&self.terrain_fingerprint);
        hasher.update(&self.forcing_fingerprint);
        hasher.update(self.climate.checkpoint().fingerprint());
        hasher.update(&self.initial_water_inventory_m3.to_bits().to_le_bytes());
        serde_json::to_writer(
            &mut hasher,
            &(
                &self.vegetation,
                &self.lakes,
                &self.rivers,
                &self.runoff_budget,
                &self.climate_quality,
            ),
        )
        .expect("finite current fields and in-memory hashing cannot fail");
        *hasher.finalize().as_bytes()
    }
}

pub(crate) fn source_corners(fine: &FineTerrain, face: usize) -> [usize; 3] {
    let indices = fine.coarse_terrain().site_triangles();
    std::array::from_fn(|i| indices[3 * face + i])
}

pub(crate) fn terrain_identity(
    fine: &FineTerrain,
    surface: &SphericalSurfaceSnapshot,
    water: &AtlasSiteWater,
    fields: &AtlasCellFields,
    depth: u8,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.atlas-terrain.v2\0");
    hasher.update(&ATLAS_TERRAIN_MODEL_VERSION.to_le_bytes());
    hasher.update(&civ_atlas_terrain::FINE_TERRAIN_VERSION.to_le_bytes());
    hasher.update(&[fine.scale().pixels_per_unit(), depth]);
    hasher.update(&surface.fingerprint());
    fine.coarse_terrain()
        .write_identity(&mut hasher)
        .expect("in-memory BLAKE3 writer is infallible");
    serde_json::to_writer(&mut hasher, water).expect("finite site classes hash in memory");
    serde_json::to_writer(&mut hasher, fields).expect("finite initialized fields hash in memory");
    *hasher.finalize().as_bytes()
}

/// Nearest-site owner, starting from the already located source triangle.
fn source_owner(
    fine: &FineTerrain,
    surface: &SphericalSurfaceSnapshot,
    sites: [usize; 3],
    direction: [f64; 3],
) -> Result<usize, TerrainError> {
    let direction = UnitVector3::new(direction[0], direction[1], direction[2])
        .map_err(|_| TerrainError::InvalidDirection)?
        .components();
    let mut best = sites[0];
    let better = |a: usize, b: usize| {
        dot(surface.cells()[a].site.components(), direction)
            .total_cmp(&dot(surface.cells()[b].site.components(), direction))
            .then_with(|| b.cmp(&a))
            .is_gt()
    };
    for site in sites {
        if better(site, best) {
            best = site;
        }
    }
    // Source Delaunay ownership walk also handles triangle-boundary tie cases.
    let terrain = fine.coarse_terrain();
    loop {
        let old = best;
        for &neighbor in &terrain.site_neighbors()
            [terrain.site_neighbor_offsets()[old]..terrain.site_neighbor_offsets()[old + 1]]
        {
            if better(neighbor, best) {
                best = neighbor;
            }
        }
        if old == best {
            return Ok(best);
        }
    }
}

/// Invalid or mismatched Atlas current-state publication.
#[derive(Debug, Error)]
pub enum AtlasTerrainValidationError {
    /// A dense array, site-class reference or finite numerical domain is invalid.
    #[error("Atlas current payload violates its numerical or allocation contract")]
    InvalidPayload,
    /// Surface and projected fields name different source content.
    #[error("Atlas source identity mismatch")]
    SourceMismatch,
    /// Published stock is not the integral of the current water geometry.
    #[error("Atlas initial water inventory mismatch")]
    WaterInventoryMismatch,
    /// Resolved routing loses water beyond the existing water-volume budget.
    #[error("Atlas resolved runoff balance mismatch")]
    RunoffMismatch,
    /// Existing C2 validity checks failed.
    #[error("Atlas climate invalid: {0}")]
    Climate(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monthly_routing_checks_each_terminal_and_the_lake_ledger() {
        let mut budget = AtlasRunoffBudget {
            monthly_runoff_m3_s: [9.0; CLIMATE_MONTH_COUNT],
            monthly_direct_lake_precipitation_m3_s: [1.0; CLIMATE_MONTH_COUNT],
            monthly_ocean_delivery_m3_s: [5.0; CLIMATE_MONTH_COUNT],
            monthly_lake_retention_m3_s: [3.0; CLIMATE_MONTH_COUNT],
            monthly_dry_depression_retention_m3_s: [2.0; CLIMATE_MONTH_COUNT],
            monthly_routing_residual_m3_s: [0.0; CLIMATE_MONTH_COUNT],
        };
        let mut lakes = [AtlasLake {
            surface_elevation_m: 10.0,
            area_m2: 100.0,
            volume_m3: 50.0,
            resolved_monthly_inflow_m3_s: [2.0; CLIMATE_MONTH_COUNT],
            resolved_monthly_outflow_m3_s: [0.0; CLIMATE_MONTH_COUNT],
            direct_monthly_precipitation_m3_s: [1.0; CLIMATE_MONTH_COUNT],
        }];
        assert!(budget.validate(&lakes).is_ok());
        budget.monthly_ocean_delivery_m3_s[4] += 1.0;
        assert!(budget.validate(&lakes).is_err());
        budget.monthly_ocean_delivery_m3_s[4] -= 1.0;
        lakes[0].resolved_monthly_outflow_m3_s[11] = 1.0;
        assert!(budget.validate(&lakes).is_err());
    }

    #[test]
    fn current_fields_reject_nonfinite_material_and_wrong_dense_allocation() {
        let mut fields = AtlasCellFields {
            land_ocean: LandOceanField::from_kinds(vec![
                crate::world::natural::LandOceanKind::Land,
            ]),
            bed_elevation_m: vec![0.0],
            exposed_elevation_m: vec![0.0],
            ocean_fraction: vec![0.0],
            lake_fraction: vec![0.0],
            dry_fraction: vec![1.0],
            ocean_volume_m3: vec![0.0],
            lake_volume_m3: vec![0.0],
            relative_permeability: vec![
                crate::world::natural::BedrockKind::ContinentalCrystalline
                    .base_transport_properties()
                    .1,
            ],
        };
        fields.validate(1).unwrap();
        assert!(fields.validate(2).is_err());
        fields.relative_permeability[0] = f32::NAN;
        assert!(matches!(
            fields.validate(1),
            Err(AtlasTerrainValidationError::InvalidPayload)
        ));
    }

    #[test]
    fn point_rule_requires_marine_connection_then_considers_the_highest_lake() {
        let lake = |lake, surface_m| Some(AtlasSiteBody::Lake { lake, surface_m });
        let mut coast = coast::CoastMap::unrefined(2);
        coast.set_marine(0);
        let water = AtlasSiteWater {
            sites: vec![Some(AtlasSiteBody::Ocean), lake(0, 5.0), lake(1, 9.0), None],
            coast,
        };
        assert_eq!(
            water.source_field(),
            SurfaceWaterField::from_kinds(vec![
                SurfaceWaterKind::Ocean,
                SurfaceWaterKind::Lake,
                SurfaceWaterKind::Lake,
                SurfaceWaterKind::DryLand,
            ])
        );
        let kind = |corners, bed, face| {
            water.classify(
                corners,
                &LocatedSurfaceSample {
                    bed_elevation_m: bed,
                    source_face: face,
                    weights: [0.25, 0.25, 0.5],
                },
            )
        };
        assert_eq!(
            kind([3, 3, 3], -1.0, 0),
            (SurfaceWaterKind::Ocean, Some(0.0), None)
        );
        // Above sea level an ocean corner yields to an adjacent lake surface.
        assert_eq!(
            kind([0, 1, 3], 2.0, 0),
            (SurfaceWaterKind::Lake, Some(5.0), Some(0))
        );
        assert_eq!(
            kind([1, 2, 3], 6.0, 1),
            (SurfaceWaterKind::Lake, Some(9.0), Some(1))
        );
        assert_eq!(kind([1, 2, 3], 9.0, 1).0, SurfaceWaterKind::DryLand);
        // Even an upstream ocean corner cannot flood a disconnected depression.
        assert_eq!(
            kind([0, 3, 3], -50.0, 1),
            (SurfaceWaterKind::DryLand, None, None)
        );
    }
}
