//! Fixed author bed and explicitly resolved current water, without fictitious P2/P3 history.

use std::sync::Arc;

use civ_atlas_terrain::{FineTerrain, TerrainError};
use serde::Serialize;
use thiserror::Error;

use super::{
    GlobalCirculationSnapshot, LandOceanField, NaturalQualityReport, SurfaceWaterKind,
    CLIMATE_MONTH_COUNT, WATER_VOLUME_RELATIVE_TOLERANCE,
};
use crate::world::spatial::{cross, dot, SphericalSurfaceSnapshot, SurfaceRef, UnitVector3};

/// Identity version for fixed-bed initialization, material prior and current runoff.
pub const ATLAS_TERRAIN_MODEL_VERSION: u32 = 1;
/// Fixed original zoom scale; camera LOD never changes the author bed.
pub const ATLAS_FINE_SCALE: civ_atlas_terrain::FineScale = civ_atlas_terrain::FineScale::Four;
/// Fixed quadrature subdivision count, audited with the production support probe.
pub const ATLAS_SUPPORT_DEPTH: u8 = 2;
/// Native validation ceiling approved on 2026-11-02 after measuring coastal
/// support demand. This is a resource limit, not a physical tolerance or a
/// cross-device availability promise; see the Task 5 measurement record.
pub const ATLAS_SUPPORT_WORKSPACE_BYTES_MAX: u64 = 4 * 1024 * 1024 * 1024;

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

#[derive(Debug, Clone)]
pub(crate) struct AtlasSupportFace {
    pub nodes: [u32; 3],
    pub owner: u32,
    pub area_m2: f64,
    pub ocean_fraction: f64,
    pub lake_fraction: f64,
    pub water_level_m: Option<f64>,
    pub lake: Option<usize>,
}
impl AtlasSupportFace {
    pub(crate) fn node_indices(&self) -> [usize; 3] {
        self.nodes.map(|id| id as usize)
    }
    pub(crate) fn owner_index(&self) -> usize {
        self.owner as usize
    }
}

#[derive(Debug)]
pub(crate) struct AtlasWaterSupport {
    pub directions: Vec<UnitVector3>,
    pub bed: Vec<f64>,
    pub faces: Vec<AtlasSupportFace>,
    pub cell_face_offsets: Vec<usize>,
}

/// A current lake initialized on the fixed bed; evaporation is not modeled by C2.
#[derive(Debug, Clone, Serialize)]
pub struct AtlasLake {
    pub(crate) surface_elevation_m: f64,
    pub(crate) spill_elevation_m: f64,
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
    /// Actual minimax escape height on the declared support graph.
    pub const fn spill_elevation_m(&self) -> f64 {
        self.spill_elevation_m
    }
    /// P1 clipped lake area, in square metres.
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
    /// Current resolved outflow, zero for a lake below its actual spill.
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

/// Current resolved channel between two support nodes, driven by Sekai precipitation.
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

/// A point query from the immutable bed and the same initialized water support.
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
    pub(crate) lowered_lake_prior_count: usize,
    pub(crate) fields: AtlasCellFields,
    pub(crate) climate: GlobalCirculationSnapshot,
    pub(crate) climate_quality: NaturalQualityReport,
    pub(crate) lakes: Vec<AtlasLake>,
    pub(crate) rivers: Vec<AtlasRiverSegment>,
    pub(crate) runoff_budget: AtlasRunoffBudget,
    #[serde(skip)]
    pub(crate) surface: Arc<SphericalSurfaceSnapshot>,
    #[serde(skip)]
    pub(crate) fine: Arc<FineTerrain>,
    #[serde(skip)]
    pub(crate) support: AtlasWaterSupport,
}
impl AtlasTerrainSnapshot {
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
    /// Count of source lake-level priors limited by the actual spill before stock initialization.
    pub const fn lowered_lake_prior_count(&self) -> usize {
        self.lowered_lake_prior_count
    }
    /// Base fan subdivisions; lake and spill cells further resolve the canonical fine interval.
    pub const fn support_depth(&self) -> u8 {
        self.support_depth
    }

    /// Sample the fixed bed and initialized connected water; rejects invalid directions.
    pub fn sample_surface(&self, direction: [f64; 3]) -> Result<AtlasSurfaceSample, TerrainError> {
        let bed = self.fine.sample_surface(direction)?.bed_elevation_m;
        self.bind_surface_sample(direction, bed)
    }

    /// Sample a caller batch with shared source scratch. Preserves order and
    /// returns no partial result on invalid input or cooperative cancellation.
    pub fn sample_surface_batch(
        &self,
        directions: &[[f64; 3]],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<AtlasSurfaceSample>, TerrainError> {
        let beds = self.fine.sample_surface_batch(directions, cancelled)?;
        directions
            .iter()
            .zip(beds)
            .map(|(&direction, bed)| {
                if cancelled() {
                    return Err(TerrainError::Cancelled);
                }
                self.bind_surface_sample(direction, bed.bed_elevation_m)
            })
            .collect()
    }

    fn bind_surface_sample(
        &self,
        direction: [f64; 3],
        bed: f64,
    ) -> Result<AtlasSurfaceSample, TerrainError> {
        bind_surface_sample(&self.fine, &self.surface, &self.support, direction, bed)
    }

    /// Check source binding, actual current stock and resolved flow conservation.
    /// Returns a typed error for any mismatched or non-finite current state.
    pub fn validate(&self) -> Result<(), AtlasTerrainValidationError> {
        self.validate_binding()?;
        let n = self.surface.cells().len();
        self.fields.validate(n)?;
        if self.fine.coarse_terrain().site_xyz().len() != 3 * n
            || self.support.bed.len() != self.support.directions.len()
            || self.support.cell_face_offsets.len() != n + 1
            || self.support.cell_face_offsets.first() != Some(&0)
            || self.support.cell_face_offsets.last() != Some(&self.support.faces.len())
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
        for (owner, offsets) in self.support.cell_face_offsets.windows(2).enumerate() {
            let faces = self
                .support
                .faces
                .get(offsets[0]..offsets[1])
                .ok_or(AtlasTerrainValidationError::InvalidPayload)?;
            if faces.is_empty()
                || faces.iter().any(|face| {
                    face.owner_index() != owner
                        || !face.area_m2.is_finite()
                        || face.area_m2 <= 0.0
                        || !(0.0..=1.0).contains(&face.ocean_fraction)
                        || !(0.0..=1.0).contains(&face.lake_fraction)
                        || face
                            .nodes
                            .iter()
                            .any(|&node| node as usize >= self.support.bed.len())
                        || face.water_level_m.is_some_and(|level| !level.is_finite())
                        || face.lake.is_some_and(|lake| lake >= self.lakes.len())
                })
            {
                return Err(AtlasTerrainValidationError::InvalidPayload);
            }
        }
        if self.support.bed.iter().any(|v| !v.is_finite()) {
            return Err(AtlasTerrainValidationError::InvalidPayload);
        }
        if self.terrain_fingerprint
            != terrain_identity(
                &self.fine,
                &self.surface,
                &self.support,
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
        if self.lakes.iter().any(|lake| {
            !lake.volume_m3.is_finite()
                || lake.volume_m3 <= 0.0
                || !lake.area_m2.is_finite()
                || lake.area_m2 <= 0.0
                || !lake.surface_elevation_m.is_finite()
                || !lake.spill_elevation_m.is_finite()
                || lake.surface_elevation_m > lake.spill_elevation_m
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
        for m in 0..CLIMATE_MONTH_COUNT {
            let initial = self.runoff_budget.monthly_runoff_m3_s[m]
                + self.runoff_budget.monthly_direct_lake_precipitation_m3_s[m];
            let residual = self.runoff_budget.monthly_routing_residual_m3_s[m];
            if !initial.is_finite()
                || !residual.is_finite()
                || residual.abs() > initial.abs() * WATER_VOLUME_RELATIVE_TOLERANCE
            {
                return Err(AtlasTerrainValidationError::RunoffMismatch);
            }
        }
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
        hasher.update(&self.terrain_fingerprint);
        hasher.update(&self.forcing_fingerprint);
        hasher.update(self.climate.checkpoint().fingerprint());
        hasher.update(&self.initial_water_inventory_m3.to_bits().to_le_bytes());
        hasher.update(&(self.lowered_lake_prior_count as u64).to_le_bytes());
        serde_json::to_writer(
            &mut hasher,
            &(
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

pub(crate) fn bind_surface_sample(
    fine: &FineTerrain,
    surface: &SphericalSurfaceSnapshot,
    support: &AtlasWaterSupport,
    direction: [f64; 3],
    bed: f64,
) -> Result<AtlasSurfaceSample, TerrainError> {
    let owner = source_owner(fine, surface, direction)?;
    let face = support_face(support, owner, direction);
    let level = face.water_level_m.filter(|&level| bed < level);
    let water = if level.is_none() {
        SurfaceWaterKind::DryLand
    } else if face.lake.is_some() {
        SurfaceWaterKind::Lake
    } else {
        SurfaceWaterKind::Ocean
    };
    Ok(AtlasSurfaceSample {
        bed_elevation_m: bed,
        water,
        water_surface_elevation_m: level,
        source_cell: owner,
    })
}

pub(crate) fn terrain_identity(
    fine: &FineTerrain,
    surface: &SphericalSurfaceSnapshot,
    support: &AtlasWaterSupport,
    fields: &AtlasCellFields,
    depth: u8,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.atlas-terrain.v1\0");
    hasher.update(&ATLAS_TERRAIN_MODEL_VERSION.to_le_bytes());
    hasher.update(&civ_atlas_terrain::FINE_TERRAIN_VERSION.to_le_bytes());
    hasher.update(&[fine.scale().pixels_per_unit(), depth]);
    hasher.update(&surface.fingerprint());
    fine.coarse_terrain()
        .write_identity(&mut hasher)
        .expect("in-memory BLAKE3 writer is infallible");
    hasher.update(&(support.directions.len() as u64).to_le_bytes());
    for point in &support.directions {
        for x in point.components() {
            hasher.update(&x.to_bits().to_le_bytes());
        }
    }
    hasher.update(&(support.bed.len() as u64).to_le_bytes());
    for bed in &support.bed {
        hasher.update(&bed.to_bits().to_le_bytes());
    }
    hasher.update(&(support.faces.len() as u64).to_le_bytes());
    for face in &support.faces {
        for n in face.nodes {
            hasher.update(&(n as u64).to_le_bytes());
        }
        hasher.update(&(face.owner as u64).to_le_bytes());
        for x in [face.area_m2, face.ocean_fraction, face.lake_fraction] {
            hasher.update(&x.to_bits().to_le_bytes());
        }
        hasher.update(&[u8::from(face.water_level_m.is_some())]);
        if let Some(level) = face.water_level_m {
            hasher.update(&level.to_bits().to_le_bytes());
        }
        hasher.update(&[u8::from(face.lake.is_some())]);
        if let Some(lake) = face.lake {
            hasher.update(&(lake as u64).to_le_bytes());
        }
    }
    hasher.update(&(support.cell_face_offsets.len() as u64).to_le_bytes());
    for &offset in &support.cell_face_offsets {
        hasher.update(&(offset as u64).to_le_bytes());
    }
    serde_json::to_writer(&mut hasher, fields).expect("finite initialized fields hash in memory");
    *hasher.finalize().as_bytes()
}

pub(crate) fn source_owner(
    fine: &FineTerrain,
    surface: &SphericalSurfaceSnapshot,
    direction: [f64; 3],
) -> Result<usize, TerrainError> {
    let (sites, _) = fine.source_triangle(direction)?;
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

pub(crate) fn support_face(
    support: &AtlasWaterSupport,
    owner: usize,
    direction: [f64; 3],
) -> &AtlasSupportFace {
    let direction = UnitVector3::new(direction[0], direction[1], direction[2])
        .expect("support lookup follows validated source direction lookup")
        .components();
    let faces =
        &support.faces[support.cell_face_offsets[owner]..support.cell_face_offsets[owner + 1]];
    // Max of the minimum oriented halfspace is deterministic on shared boundaries,
    // including roundoff-sized negative scores; no geographic tolerance is invented.
    faces
        .iter()
        .max_by(|a, b| {
            let score = |face: &AtlasSupportFace| {
                let p = face
                    .node_indices()
                    .map(|i| support.directions[i].components());
                (0..3)
                    .map(|i| dot(cross(p[i], p[(i + 1) % 3]), direction))
                    .fold(f64::INFINITY, f64::min)
            };
            score(a).total_cmp(&score(b))
        })
        .expect("validated source cells contain support fan triangles")
}

/// Invalid or mismatched Atlas current-state publication.
#[derive(Debug, Error)]
pub enum AtlasTerrainValidationError {
    /// A dense array, support reference or finite numerical domain is invalid.
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
}
