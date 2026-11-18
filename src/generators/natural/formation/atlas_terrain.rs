//! Fixed-bed Civ Atlas source with upstream water classes, coupled to C2
//! (`2026-11-03-world-truth-scope-design.md` R3).

#[cfg(test)]
mod carrier_probe;
mod climate;
mod coast;
#[cfg(test)]
mod coast_regression;
mod geometry;
mod runoff;

use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts,
    BuildCancellation, Diagnostic, GraphError, Stage, StageError, StageGraph, StageGraphBuilder,
    StageId, StageInputs, StageRng,
};
use crate::generators::spatial::build_source_voronoi_surface;
use crate::world::natural::{
    AtlasTerrainSnapshot, AtlasTerrainValidationError, ClimateModelProfile, ClimateSpec,
    HydroErosionSpec, NaturalQualityProfile, ATLAS_FINE_SCALE, ATLAS_SUPPORT_DEPTH,
    ATLAS_TERRAIN_MODEL_VERSION, EARTH_WATER_REFERENCE_RADIUS_M,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, UnitVector3};
use crate::world::Meters;
use civ_atlas_terrain::{generate_terrain, FineScale, FineTerrain, TerrainParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// Source-author controls and current Sekai climate/routing controls, without a P2/P3 history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AtlasTerrainRequest {
    /// Original source author controls, including the reproducible source seed.
    pub author: TerrainParams,
    /// Current climate controls; these do not replace private source author weather.
    pub climate: ClimateSpec,
    /// Current channel publication threshold; no erosion time is advanced.
    pub hydrology: HydroErosionSpec,
    /// Existing C2 work-grid quality selection.
    pub quality: NaturalQualityProfile,
}
impl Default for AtlasTerrainRequest {
    fn default() -> Self {
        Self {
            author: TerrainParams::default(),
            climate: ClimateSpec::default(),
            hydrology: HydroErosionSpec::default(),
            quality: NaturalQualityProfile::Draft,
        }
    }
}
impl AtlasTerrainRequest {
    /// Validate author and current-process controls before allocating a source mesh.
    /// Returns an input error for unsupported values or climate quality.
    pub fn validate(&self) -> Result<(), AtlasGenerationError> {
        self.author.validate()?;
        self.climate
            .validate()
            .map_err(|e| AtlasGenerationError::Geometry(e.to_string()))?;
        self.hydrology
            .validate()
            .map_err(|e| AtlasGenerationError::Geometry(e.to_string()))?;
        Ok(())
    }
}

/// Builds one actual fixed-bed source → C2 → current-routing product.
pub struct AtlasTerrainGenerator;
impl AtlasTerrainGenerator {
    /// Generate an atomically complete current state from a recipe's author seed.
    ///
    /// Errors and cancellation publish no intermediate terrain, water or climate.
    /// Lake evaporation/groundwater and solid evolution remain explicitly unmodeled.
    pub fn generate(
        request: &AtlasTerrainRequest,
        cancellation: &BuildCancellation,
    ) -> Result<AtlasTerrainArtifact, AtlasGenerationError> {
        request.validate()?;
        check_cancelled(cancellation)?;
        let phase = crate::engine::StageTimer::now();
        let (fine, surface) = prepare_source(&request.author, ATLAS_FINE_SCALE, cancellation)?;
        log::debug!("Atlas source/fine ms={}", phase.elapsed().as_millis());
        let phase = crate::engine::StageTimer::now();
        let geometry =
            geometry::build_geometry(&fine, &surface, ATLAS_SUPPORT_DEPTH, cancellation)?;
        log::debug!(
            "Atlas cell water quadrature ms={}",
            phase.elapsed().as_millis()
        );
        finish_current(
            request,
            (fine, surface, geometry),
            ATLAS_SUPPORT_DEPTH,
            cancellation,
        )
    }
}

fn finish_current(
    request: &AtlasTerrainRequest,
    prepared: (
        Arc<FineTerrain>,
        Arc<SphericalSurfaceSnapshot>,
        geometry::BuiltGeometry,
    ),
    depth: u8,
    cancellation: &BuildCancellation,
) -> Result<AtlasTerrainArtifact, AtlasGenerationError> {
    let (fine, surface, mut geometry) = prepared;
    let phase = crate::engine::StageTimer::now();
    let terrain_fingerprint = crate::world::natural::atlas_terrain::terrain_identity(
        &fine,
        &surface,
        &geometry.water,
        &geometry.fields,
        depth,
    );
    log::debug!("Atlas terrain identity ms={}", phase.elapsed().as_millis());
    let phase = crate::engine::StageTimer::now();
    let domain = super::ClimateWorkDomainBuilder::build(&surface, request.quality, cancellation)?;
    let forcing = climate::build_forcing(
        (&fine, &surface, &geometry, terrain_fingerprint),
        &domain,
        &request.climate,
        cancellation,
    )?;
    log::debug!("Atlas domain/forcing ms={}", phase.elapsed().as_millis());
    let phase = crate::engine::StageTimer::now();
    let climate = super::GlobalCirculationGenerator::generate_from_validated(
        &surface,
        &domain,
        &forcing,
        ClimateModelProfile::C2LayeredV1,
        cancellation,
    )?;
    log::debug!("Atlas C2 ms={}", phase.elapsed().as_millis());
    let phase = crate::engine::StageTimer::now();
    let climate_quality = crate::generators::natural::quality::evaluate_global_circulation_quality_for_atlas_cancellable(
        &surface, (&geometry.fields, terrain_fingerprint), &forcing, &climate, cancellation)?;
    crate::generators::natural::quality::validate_global_circulation_quality_report(
        &climate_quality,
        crate::world::spatial::SurfaceRef::for_spherical(&surface),
        climate.checkpoint().fingerprint(),
    )
    .map_err(AtlasGenerationError::Geometry)?;
    let (rivers, runoff_budget) = runoff::build_current_runoff(
        fine.coarse_terrain(),
        &surface,
        &mut geometry,
        &climate,
        &request.hydrology,
        cancellation,
    )?;
    log::debug!(
        "Atlas quality/current routing ms={}",
        phase.elapsed().as_millis()
    );
    let phase = crate::engine::StageTimer::now();
    let initial_water_inventory_m3 = geometry
        .fields
        .ocean_volume_m3
        .iter()
        .chain(&geometry.fields.lake_volume_m3)
        .sum();
    let mut snapshot = AtlasTerrainSnapshot {
        fingerprint: [0; 32],
        terrain_fingerprint,
        forcing_fingerprint: *forcing.fingerprint(),
        surface_ref: crate::world::spatial::SurfaceRef::for_spherical(&surface),
        support_depth: depth,
        initial_water_inventory_m3,
        fields: geometry.fields,
        climate,
        climate_quality,
        lakes: geometry.lakes,
        rivers,
        runoff_budget,
        surface,
        fine,
        site_water: geometry.water,
    };
    snapshot.fingerprint = snapshot.calculate_fingerprint();
    snapshot.validate()?;
    log::debug!(
        "Atlas current seal/full validation ms={}",
        phase.elapsed().as_millis()
    );
    check_cancelled(cancellation)?;
    Ok(AtlasTerrainArtifact { snapshot })
}

/// In-memory validated current product; its fine evaluator cannot be deserialized as a cache.
#[derive(Debug)]
pub struct AtlasTerrainArtifact {
    snapshot: AtlasTerrainSnapshot,
}
impl Serialize for AtlasTerrainArtifact {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (
            Self::KEY.as_str(),
            ATLAS_TERRAIN_MODEL_VERSION,
            self.snapshot.fingerprint(),
        )
            .serialize(serializer)
    }
}
impl AtlasTerrainArtifact {
    /// Borrow the complete current terrain, water, climate and channel state.
    pub const fn snapshot(&self) -> &AtlasTerrainSnapshot {
        &self.snapshot
    }
}
impl Artifact for AtlasTerrainArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.atlas-terrain-current");
    fn validate(&self) -> Result<(), ArtifactValidationError> {
        // The sole constructor fully validates the actual fine/support/current
        // contents before wrapping them. There is no Deserialize, mutable getter
        // or public from-snapshot route. Engine publication therefore rechecks
        // the immutable binding, rather than rehashing all geometry and climate.
        self.snapshot
            .validate_binding()
            .map_err(|e| ArtifactValidationError::new("atlas.invalid-current", e.to_string()))
    }
}

/// Strict external input for the independent Atlas generation graph.
#[derive(Debug, Clone, Serialize)]
pub struct AtlasTerrainRequestArtifact {
    request: AtlasTerrainRequest,
}
impl AtlasTerrainRequestArtifact {
    /// Validate a recipe before it becomes an engine input; invalid controls fail here.
    pub fn new(request: AtlasTerrainRequest) -> Result<Self, AtlasGenerationError> {
        request.validate()?;
        Ok(Self { request })
    }
    /// Borrow the validated source/current-process request.
    pub const fn request(&self) -> &AtlasTerrainRequest {
        &self.request
    }
}
impl Artifact for AtlasTerrainRequestArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.atlas-terrain-request");
    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.request
            .validate()
            .map_err(|e| ArtifactValidationError::new("atlas.invalid-request", e.to_string()))
    }
}

/// Engine dependencies of the independent fixed-bed source producer.
pub struct AtlasTerrainStageInputs {
    request: Arc<AtlasTerrainRequestArtifact>,
}
impl StageInputs for AtlasTerrainStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[AtlasTerrainRequestArtifact::KEY]
    }
    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            request: artifacts.get::<AtlasTerrainRequestArtifact>()?,
        })
    }
}
/// One atomic Atlas/C2/current-water publication stage.
#[derive(Debug, Default)]
pub struct AtlasTerrainStage;
impl Stage for AtlasTerrainStage {
    type Inputs = AtlasTerrainStageInputs;
    type Output = AtlasTerrainArtifact;
    fn id(&self) -> StageId {
        StageId::new("natural.atlas-terrain-current")
    }
    fn version(&self) -> u32 {
        ATLAS_TERRAIN_MODEL_VERSION
    }
    fn namespace(&self) -> &'static str {
        "sekai.civ-atlas"
    }
    fn run(
        &self,
        inputs: Self::Inputs,
        rng: &mut StageRng,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        let cancellation = rng.cancellation_signal();
        AtlasTerrainGenerator::generate(inputs.request.request(), &cancellation).map_err(|error| {
            StageError::new(
                if cancellation.is_cancelled() {
                    "engine.cancelled"
                } else {
                    "atlas.generation-failed"
                },
                error.to_string(),
            )
        })
    }
}
/// Independent source graph; it never requests or fabricates a P2/P3 artifact.
/// Returns the engine's graph-validation error if its static declaration is inconsistent.
pub fn atlas_terrain_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<AtlasTerrainRequestArtifact>()
        .stage(AtlasTerrainStage)
        .build()
}

pub(super) fn check_cancelled(
    cancellation: &BuildCancellation,
) -> Result<(), AtlasGenerationError> {
    if cancellation.is_cancelled() {
        Err(AtlasGenerationError::Cancelled)
    } else {
        Ok(())
    }
}

fn prepare_source(
    params: &TerrainParams,
    scale: FineScale,
    cancellation: &BuildCancellation,
) -> Result<(Arc<FineTerrain>, Arc<SphericalSurfaceSnapshot>), AtlasGenerationError> {
    let terrain = Arc::new(generate_terrain(params, &|| cancellation.is_cancelled())?);
    let directions = terrain
        .site_xyz()
        .chunks_exact(3)
        .map(|p| {
            UnitVector3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
                .map_err(|e| AtlasGenerationError::Geometry(e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let surface = build_source_voronoi_surface(
        Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).expect("Earth reference radius is positive"),
        &directions,
        terrain.site_triangles(),
        &mut || cancellation.is_cancelled(),
    )?;
    let fine = FineTerrain::new(terrain, scale, &|| cancellation.is_cancelled())?;
    Ok((Arc::new(fine), Arc::new(surface)))
}

/// A point a fraction of the way along a non-degenerate minor great-circle arc.
fn minor_arc_direction(a: UnitVector3, b: UnitVector3, angle: f64, fraction: f64) -> [f64; 3] {
    let left = ((1.0 - fraction) * angle).sin() / angle.sin();
    let right = (fraction * angle).sin() / angle.sin();
    let p: [f64; 3] = std::array::from_fn(|j| left * a.components()[j] + right * b.components()[j]);
    UnitVector3::new(p[0], p[1], p[2])
        .expect("valid minor arc is finite")
        .components()
}

/// A source, geometry, climate or current-flow error prevents atomic publication.
#[derive(Debug, Error)]
pub enum AtlasGenerationError {
    /// The owning build requested cancellation.
    #[error("Atlas generation cancelled")]
    Cancelled,
    /// The original source controls or canonical evaluator failed.
    #[error(transparent)]
    Terrain(#[from] civ_atlas_terrain::TerrainError),
    /// The source mesh did not satisfy Sekai's spherical geometry contract.
    #[error(transparent)]
    Surface(#[from] crate::generators::spatial::SphericalSurfaceBuildError),
    /// The existing hydrology kernel rejected the source-site routing.
    #[error(transparent)]
    Hydrology(#[from] crate::generators::natural::HydrologyGenerationError),
    /// The upstream classes or derived cell fields are inconsistent.
    #[error("Atlas geometry: {0}")]
    Geometry(String),
    /// Existing C2 boundary validation failed.
    #[error(transparent)]
    Forcing(#[from] super::GlobalClimateForcingError),
    /// The current C2 work-grid or remapping could not be constructed.
    #[error(transparent)]
    WorkDomain(#[from] super::ClimateWorkDomainBuildError),
    /// The actual C2 solve failed its existing numerical or physical contracts.
    #[error(transparent)]
    Climate(#[from] super::GlobalCirculationGenerationError),
    /// Current state binding or declared budgets are inconsistent.
    #[error(transparent)]
    Validation(#[from] AtlasTerrainValidationError),
    /// Existing production climate quality evidence failed validation.
    #[error(transparent)]
    Quality(#[from] crate::generators::natural::QualityBuildError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::BuildCancellation;
    use crate::world::natural::CLIMATE_MONTH_COUNT;

    #[test]
    fn missing_source_and_cancelled_requests_do_not_construct_a_world() {
        assert!(AtlasTerrainStageInputs::load(&BuildArtifacts::default()).is_err());
        let cancellation = BuildCancellation::new();
        cancellation.cancel();
        assert!(matches!(
            AtlasTerrainGenerator::generate(&AtlasTerrainRequest::default(), &cancellation),
            Err(AtlasGenerationError::Cancelled)
        ));
    }

    #[test]
    fn cancellation_during_source_construction_publishes_no_artifact() {
        let cancellation = BuildCancellation::new();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                AtlasTerrainGenerator::generate(&AtlasTerrainRequest::default(), &cancellation)
            });
            while cancellation.observation_count() < 10 && !worker.is_finished() {
                std::hint::spin_loop();
            }
            assert!(cancellation.observation_count() >= 10);
            cancellation.cancel();
            assert!(matches!(
                worker.join().unwrap(),
                Err(AtlasGenerationError::Cancelled)
                    | Err(AtlasGenerationError::Terrain(
                        civ_atlas_terrain::TerrainError::Cancelled
                    ))
            ));
        });
    }

    #[test]
    fn coastal_fraction_runoff_and_evapotranspiration_use_the_same_dry_area() {
        let permeability = crate::world::natural::BedrockKind::ContinentalCrystalline
            .base_transport_properties()
            .1;
        let whole = runoff::dry_runoff_rate_m3_s(10.0, 200.0, permeability);
        let coastal = runoff::dry_runoff_rate_m3_s(10.0, 50.0, permeability);
        assert_eq!(coastal, whole / 4.0);
        assert_eq!(runoff::dry_runoff_rate_m3_s(10.0, 0.0, permeability), 0.0);
        let fraction = crate::world::natural::formation_runoff_fraction(permeability);
        assert_eq!(
            coastal / fraction,
            10.0 * 50.0 / 1000.0 / crate::world::natural::MEAN_SOLAR_DAY_SECONDS
        );
    }

    /// One actual default-world publication: phase timings (debug log), full
    /// audit, tamper detection, point-query invariance and the consistency
    /// of picked samples with the published cell fields.
    #[test]
    #[ignore = "one actual Atlas to C2/current-water publication; Release only"]
    fn atlas_canonical_current_probe() {
        let _ = env_logger::Builder::new()
            .filter_module(
                "sekai::generators::natural::formation::atlas_terrain",
                log::LevelFilter::Debug,
            )
            .is_test(true)
            .try_init();
        let cancellation = BuildCancellation::new();
        let request = AtlasTerrainRequest {
            author: TerrainParams {
                seed: 42,
                ..Default::default()
            },
            ..Default::default()
        };
        let start = std::time::Instant::now();
        let mut artifact = AtlasTerrainGenerator::generate(&request, &cancellation).unwrap();
        let generate_ms = start.elapsed().as_millis();
        let audit_start = std::time::Instant::now();
        artifact.snapshot.validate().unwrap();
        let audit_ms = audit_start.elapsed().as_millis();

        let snapshot = &artifact.snapshot;
        eprintln!(
            "ATLAS current fingerprint={}",
            blake3::Hash::from(*snapshot.fingerprint())
        );
        let unit = [1.0, 0.5, 0.25];
        let large = unit.map(|v| v * f64::MAX);
        let samples = snapshot
            .sample_surface_batch(&[unit, large], &|| false)
            .unwrap();
        assert_eq!(
            samples[0].bed_elevation_m.to_bits(),
            samples[1].bed_elevation_m.to_bits()
        );
        assert_eq!(samples[0].source_cell, samples[1].source_cell);
        assert_eq!(samples[0].water, samples[1].water);
        assert!(matches!(
            snapshot.sample_surface_batch(&[unit], &|| true),
            Err(civ_atlas_terrain::TerrainError::Cancelled)
        ));
        // Picking and cell fields share one rule: a site that the fields call
        // wholly ocean must pick as ocean wherever its bed is below sea level.
        let fields = snapshot.fields();
        let pick_mismatch = snapshot
            .surface()
            .cells()
            .iter()
            .enumerate()
            .filter(|&(i, _)| fields.ocean_fraction()[i] == 1.0)
            .map(|(_, cell)| snapshot.sample_surface(cell.site.components()).unwrap())
            .filter(|s| s.bed_elevation_m < 0.0)
            .filter(|s| s.water != crate::world::natural::SurfaceWaterKind::Ocean)
            .count();
        assert_eq!(pick_mismatch, 0);
        let area = |f: &[f32]| -> f64 {
            snapshot
                .surface()
                .cells()
                .iter()
                .zip(f)
                .map(|(c, &v)| c.area.get() * f64::from(v))
                .sum()
        };
        eprintln!(
            "ATLAS current total_ms={} lakes={} rivers={} ocean_km2={:.0} lake_km2={:.1} \
             initial_water_m3={:e} ocean_m3={:e} lake_m3={:e} budget={:?} climate={:?}",
            start.elapsed().as_millis(),
            snapshot.lakes().len(),
            snapshot.rivers().len(),
            area(fields.ocean_fraction()) / 1e6,
            area(fields.lake_fraction()) / 1e6,
            snapshot.initial_water_inventory_m3(),
            fields.ocean_volume_m3().iter().sum::<f64>(),
            fields.lake_volume_m3().iter().sum::<f64>(),
            snapshot.runoff_budget(),
            snapshot.climate().budget_report()
        );
        for (i, lake) in snapshot.lakes().iter().enumerate() {
            eprintln!(
                "ATLAS lake={i} surface_m={:.1} area_km2={:.1} volume_km3={:.3} \
                 annual_net_input_m3_s={:.1}",
                lake.surface_elevation_m(),
                lake.area_m2() / 1e6,
                lake.volume_m3() / 1e9,
                lake.monthly_resolved_net_input_m3_s().iter().sum::<f64>()
                    / CLIMATE_MONTH_COUNT as f64
            );
        }
        let suggestions = snapshot
            .climate_quality()
            .metrics()
            .iter()
            .filter(|metric| metric.status() != crate::world::natural::QualityMetricStatus::Pass)
            .map(|metric| (metric.id().name(), metric.status()))
            .collect::<Vec<_>>();
        eprintln!("ATLAS climate suggestions={suggestions:?}");

        let snapshot = &mut artifact.snapshot;
        let inventory = snapshot.initial_water_inventory_m3;
        snapshot.initial_water_inventory_m3 *= 1.01;
        assert!(matches!(
            snapshot.validate(),
            Err(AtlasTerrainValidationError::WaterInventoryMismatch)
        ));
        snapshot.initial_water_inventory_m3 = inventory;
        snapshot.support_depth += 1;
        assert!(matches!(
            snapshot.validate(),
            Err(AtlasTerrainValidationError::SourceMismatch)
        ));
        snapshot.support_depth -= 1;
        snapshot.terrain_fingerprint[0] ^= 1;
        assert!(matches!(
            snapshot.validate(),
            Err(AtlasTerrainValidationError::SourceMismatch)
        ));
        snapshot.terrain_fingerprint[0] ^= 1;
        snapshot.validate().unwrap();

        let publication = std::time::Instant::now();
        let mut stored = BuildArtifacts::default();
        stored.insert(artifact).unwrap();
        eprintln!(
            "ATLAS timing generate_ms={generate_ms} explicit_full_audit_ms={audit_ms} \
             engine_validate_hash_ms={}",
            publication.elapsed().as_millis()
        );
    }
}
