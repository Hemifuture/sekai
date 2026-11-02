//! Fixed-bed Civ Atlas source, physically initialized before C2 publication.

mod climate;
mod geometry;
mod red_refinement;
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
        log::debug!("Atlas base geometry ms={}", phase.elapsed().as_millis());
        let phase = crate::engine::StageTimer::now();
        let geometry = geometry::refine_water_support(&fine, &surface, geometry, cancellation)?;
        log::debug!(
            "Atlas canonical water geometry ms={}",
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
        &geometry.support,
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
    let (rivers, runoff_budget) =
        runoff::build_current_runoff(&mut geometry, &climate, &request.hydrology, cancellation)?;
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
        lowered_lake_prior_count: geometry.water.lowered_priors,
        fields: geometry.fields,
        climate,
        climate_quality,
        lakes: geometry.lakes,
        rivers,
        runoff_budget,
        surface,
        fine,
        support: geometry.support,
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
    /// The existing hydrology kernel rejected the support topology.
    #[error(transparent)]
    Hydrology(#[from] crate::generators::natural::HydrologyGenerationError),
    /// The initialized geometry is inconsistent.
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
    /// The support workspace would exceed the existing per-map memory ceiling.
    #[error("Atlas support requires {required} bytes, exceeding budget {maximum}")]
    SupportBudget {
        /// Conservative peak workspace estimate before allocation.
        required: u64,
        /// Existing bounded natural-map workspace ceiling.
        maximum: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::geometry::{initialize_water, OrderedHeight};
    use super::*;
    use crate::engine::BuildCancellation;

    fn graph(adjacency: Vec<Vec<usize>>) -> geometry::SupportGraph {
        geometry::SupportGraph::from_edges(
            adjacency.len(),
            adjacency
                .iter()
                .enumerate()
                .flat_map(|(i, row)| row.iter().map(move |&j| [i, j])),
            &BuildCancellation::new(),
        )
        .unwrap()
    }

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
    fn negative_inland_depression_is_not_an_ocean_terminal() {
        // Ocean 0--1, a positive sill 2, and a below-sea-level inland lake 3.
        let neighbors = graph(vec![vec![1], vec![0, 2], vec![1, 3], vec![2]]);
        let bed = [-20.0, -10.0, 5.0, -3.0];
        let water = initialize_water(
            &bed,
            &neighbors,
            &[true, false, false, false],
            &[(3, -1.0)],
            &BuildCancellation::new(),
        )
        .unwrap();
        assert_eq!(water.ocean, vec![true, true, false, false]);
        assert_eq!(water.level[3], Some(-1.0));
        assert_eq!(water.spill[3], OrderedHeight(5.0));
    }

    #[test]
    fn independent_source_seas_survive_but_an_unseeded_negative_pocket_is_dry() {
        let neighbors = graph(vec![vec![1], vec![0, 2], vec![1, 3], vec![2, 4], vec![3]]);
        let bed = [-10.0, 2.0, -8.0, 3.0, -2.0];
        let water = initialize_water(
            &bed,
            &neighbors,
            &[true, false, true, false, false],
            &[],
            &BuildCancellation::new(),
        )
        .unwrap();
        assert_eq!(water.ocean, [true, false, true, false, false]);
    }

    #[test]
    fn lake_initialization_caps_at_real_spill_and_merges_without_order_dependence() {
        let neighbors = graph(vec![vec![1], vec![0, 2], vec![1, 3], vec![2, 4], vec![3]]);
        let bed = [-10.0, 5.0, 0.0, 1.0, 0.0];
        let a = initialize_water(
            &bed,
            &neighbors,
            &[true, false, false, false, false],
            &[(2, 7.0), (4, 2.0)],
            &BuildCancellation::new(),
        )
        .unwrap();
        let b = initialize_water(
            &bed,
            &neighbors,
            &[true, false, false, false, false],
            &[(4, 2.0), (2, 7.0)],
            &BuildCancellation::new(),
        )
        .unwrap();
        assert_eq!(a.level, b.level);
        assert_eq!(a.level[2], Some(5.0));
        assert_eq!(a.level[4], Some(5.0));
        assert_eq!(a.lake[2], a.lake[4]);
        assert_eq!(a.level[1], None);
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

    #[test]
    #[ignore = "one bounded production-operator integration measurement; Release only"]
    fn atlas_support_integration_probe() {
        use std::time::Instant;
        let cancellation = BuildCancellation::new();
        let params = TerrainParams {
            seed: 42,
            ..TerrainParams::default()
        };
        let start = Instant::now();
        let (fine, surface) = prepare_source(&params, FineScale::Four, &cancellation).unwrap();
        eprintln!(
            "ATLAS source_ms={} cells={} maximum_degree={}",
            start.elapsed().as_millis(),
            surface.cells().len(),
            surface
                .cells()
                .iter()
                .map(|c| c.boundary_vertices.len())
                .max()
                .unwrap()
        );
        let mut previous: Option<crate::world::natural::AtlasCellFields> = None;
        for depth in 0..=2 {
            let start = Instant::now();
            let geometry = geometry::build_geometry(&fine, &surface, depth, &cancellation).unwrap();
            let areas = surface
                .cells()
                .iter()
                .map(|c| c.area.get())
                .collect::<Vec<_>>();
            let ocean_area = geometry
                .fields
                .ocean_fraction
                .iter()
                .zip(&areas)
                .map(|(&f, a)| f64::from(f) * a)
                .sum::<f64>();
            eprintln!("ATLAS depth={} samples={} faces={} ms={} ocean_area={} ocean_volume={} lake_area={} lake_volume={} lakes={} lowered={}",
                depth, geometry.support.directions.len(), geometry.support.faces.len(), start.elapsed().as_millis(),
                ocean_area, geometry.fields.ocean_volume_m3.iter().sum::<f64>(),
                geometry.lakes.iter().map(|l| l.area_m2).sum::<f64>(),
                geometry.lakes.iter().map(|l| l.volume_m3).sum::<f64>(), geometry.lakes.len(), geometry.water.lowered_priors);
            if let Some(previous) = &previous {
                let ocean_area_change = previous
                    .ocean_fraction
                    .iter()
                    .zip(&geometry.fields.ocean_fraction)
                    .zip(&areas)
                    .map(|((&a, &b), area)| f64::from((a - b).abs()) * area)
                    .sum::<f64>();
                let volume_change = previous
                    .ocean_volume_m3
                    .iter()
                    .zip(&geometry.fields.ocean_volume_m3)
                    .map(|(a, b)| (a - b).abs())
                    .sum::<f64>();
                eprintln!(
                    "ATLAS adjacent depth={} ocean_area_l1={} ocean_volume_l1={}",
                    depth, ocean_area_change, volume_change
                );
            }
            // Same source windows used by the original fine oracle, now comparing
            // the actual spherical parent integrals, not projected pixel counts.
            for (name, cx, cy) in [
                ("mouth", 560.0, 500.0),
                ("lake", 191.0, 680.0),
                ("seam", 0.0, 512.0),
            ] {
                let cells = surface
                    .cells()
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| {
                        let p = c.site.components();
                        let x = (p[1].atan2(p[0]) + std::f64::consts::PI) / std::f64::consts::TAU
                            * 2048.0;
                        let y = (std::f64::consts::FRAC_PI_2 - p[2].asin()) / std::f64::consts::PI
                            * 1024.0;
                        let dx = (x - cx + 1024.0).rem_euclid(2048.0) - 1024.0;
                        dx.abs() <= 16.0 && (y - cy).abs() <= 16.0
                    })
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>();
                let sum = |field: &[f64]| cells.iter().map(|&i| field[i]).sum::<f64>();
                let fraction_area = |field: &[f32]| {
                    cells
                        .iter()
                        .map(|&i| f64::from(field[i]) * areas[i])
                        .sum::<f64>()
                };
                eprintln!("ATLAS window={} depth={} cells={} ocean_area={} ocean_volume={} lake_area={} lake_volume={}", name, depth, cells.len(),
                    fraction_area(&geometry.fields.ocean_fraction), sum(&geometry.fields.ocean_volume_m3),
                    fraction_area(&geometry.fields.lake_fraction), sum(&geometry.fields.lake_volume_m3));
            }
            previous = Some(geometry.fields.clone());
            if depth == 2 {
                let start = Instant::now();
                let request = AtlasTerrainRequest {
                    author: params.clone(),
                    ..Default::default()
                };
                let mut artifact = finish_current(
                    &request,
                    (Arc::clone(&fine), Arc::clone(&surface), geometry),
                    depth,
                    &cancellation,
                )
                .unwrap();
                eprintln!("ATLAS climate_and_current_ms={} rivers={} climate_budget={:?} runoff_budget={:?}",
                    start.elapsed().as_millis(), artifact.snapshot.rivers().len(), artifact.snapshot.climate().budget_report(),
                    artifact.snapshot.runoff_budget());
                artifact.snapshot.validate().unwrap();
                let inventory = artifact.snapshot.initial_water_inventory_m3;
                artifact.snapshot.initial_water_inventory_m3 *= 1.01;
                assert!(matches!(
                    artifact.snapshot.validate(),
                    Err(AtlasTerrainValidationError::WaterInventoryMismatch)
                ));
                artifact.snapshot.initial_water_inventory_m3 = inventory;
                artifact.snapshot.support_depth += 1;
                assert!(matches!(
                    artifact.snapshot.validate(),
                    Err(AtlasTerrainValidationError::SourceMismatch)
                ));
                artifact.snapshot.support_depth -= 1;
                artifact.snapshot.terrain_fingerprint[0] ^= 1;
                assert!(matches!(
                    artifact.snapshot.validate(),
                    Err(AtlasTerrainValidationError::SourceMismatch)
                ));
            }
        }
    }

    #[test]
    #[ignore = "local production-operator lake resolution audit; Release only"]
    fn atlas_lake_support_probe() {
        use crate::generators::natural::surface_water_geometry::integrate_positive_linear_triangle;
        use crate::world::spatial::spherical_triangle_area_unit;
        use rustc_hash::FxHashMap;
        use std::collections::VecDeque;
        let cancellation = BuildCancellation::new();
        let (fine, surface) = prepare_source(
            &TerrainParams {
                seed: 42,
                ..Default::default()
            },
            FineScale::Four,
            &cancellation,
        )
        .unwrap();
        for prior in fine
            .lake_priors()
            .iter()
            .filter(|p| p.site == 10689 || p.site == 23710)
        {
            let terrain = fine.coarse_terrain();
            let mut owners = vec![prior.site];
            owners.extend_from_slice(
                &terrain.site_neighbors()[terrain.site_neighbor_offsets()[prior.site]
                    ..terrain.site_neighbor_offsets()[prior.site + 1]],
            );
            let (all_directions, all_triangles) = geometry::source_fans(&surface);
            let mut remap = FxHashMap::default();
            let mut directions = Vec::new();
            let mut triangles = all_triangles
                .into_iter()
                .filter(|(_, owner)| owners.contains(owner))
                .map(|(nodes, owner)| {
                    (
                        nodes.map(|old| {
                            *remap.entry(old).or_insert_with(|| {
                                let id = directions.len();
                                directions.push(all_directions[old]);
                                id
                            })
                        }),
                        owner,
                    )
                })
                .collect::<Vec<_>>();
            let root = remap[&prior.site];
            let mut bed = Vec::new();
            for depth in 0..=6 {
                let start = std::time::Instant::now();
                if depth > 0 {
                    triangles =
                        geometry::refine_triangles(&mut directions, triangles, &cancellation)
                            .unwrap();
                }
                for chunk in directions[bed.len()..].chunks(4096) {
                    bed.extend(
                        fine.sample_surface_batch(
                            &chunk.iter().map(|p| p.components()).collect::<Vec<_>>(),
                            &|| false,
                        )
                        .unwrap()
                        .into_iter()
                        .map(|p| p.bed_elevation_m),
                    );
                }
                let mut adjacency = vec![Vec::new(); directions.len()];
                let mut edges = FxHashMap::<[usize; 2], usize>::default();
                for &(nodes, _) in &triangles {
                    for k in 0..3 {
                        let a = nodes[k];
                        let b = nodes[(k + 1) % 3];
                        adjacency[a].push(b);
                        adjacency[b].push(a);
                        *edges.entry([a.min(b), a.max(b)]).or_default() += 1;
                    }
                }
                let mut wet = vec![false; directions.len()];
                wet[root] = bed[root] < prior.surface_elevation_m;
                let mut queue = if wet[root] {
                    VecDeque::from([root])
                } else {
                    VecDeque::new()
                };
                while let Some(i) = queue.pop_front() {
                    for &j in &adjacency[i] {
                        if !wet[j] && bed[j] < prior.surface_elevation_m {
                            wet[j] = true;
                            queue.push_back(j);
                        }
                    }
                }
                let touches_boundary = edges
                    .iter()
                    .any(|(edge, &count)| count == 1 && edge.iter().any(|&i| wet[i]));
                let mut area = 0.0;
                let mut volume = 0.0;
                for &(nodes, _) in &triangles {
                    if !nodes.iter().any(|&i| wet[i]) {
                        continue;
                    }
                    let [a, b, c] = nodes.map(|i| directions[i]);
                    let triangle_area =
                        spherical_triangle_area_unit(a, b, c) * surface.radius().get().powi(2);
                    let (fraction, depth_integral) = integrate_positive_linear_triangle(
                        nodes.map(|i| prior.surface_elevation_m - bed[i]),
                    );
                    area += fraction * triangle_area;
                    volume += depth_integral * triangle_area;
                }
                eprintln!("ATLAS local_lake={} depth={} samples={} ms={} area={} volume={} touches_boundary={}",
                    prior.site, depth, directions.len(), start.elapsed().as_millis(), area, volume, touches_boundary);
            }
        }
    }

    #[test]
    #[ignore = "diagnose actual default-world terminal capacities without rerunning climate"]
    fn atlas_current_terminal_capacity_probe() {
        let cancellation = BuildCancellation::new();
        let (fine, surface) = prepare_source(
            &TerrainParams {
                seed: 42,
                ..Default::default()
            },
            FineScale::Four,
            &cancellation,
        )
        .unwrap();
        let geometry = geometry::build_geometry(&fine, &surface, 2, &cancellation).unwrap();
        let routing = runoff::current_routing(&geometry, &cancellation).unwrap();
        let mut flat = 0;
        let mut positive = 0;
        let mut minimum_positive_depth = f64::INFINITY;
        let mut maximum_positive_depth: f64 = 0.0;
        for (i, receiver) in routing.receiver.iter().enumerate() {
            if receiver.is_some() || geometry.water.ocean[i] || geometry.water.lake[i].is_some() {
                continue;
            }
            let deficit = geometry.water.spill[i].0 - geometry.support.bed[i];
            if deficit == 0.0 {
                flat += 1;
            } else {
                assert!(deficit > 0.0);
                positive += 1;
                minimum_positive_depth = minimum_positive_depth.min(deficit);
                maximum_positive_depth = maximum_positive_depth.max(deficit);
            }
        }
        eprintln!(
            "ATLAS dry_terminals flat={} positive_capacity={} positive_spill_deficit_m=[{},{}]",
            flat, positive, minimum_positive_depth, maximum_positive_depth
        );
        assert_eq!(
            flat, 0,
            "outlet-connected zero-capacity flats must drain without invented water"
        );
        for prior in fine.lake_priors() {
            eprintln!(
                "ATLAS prior={} original_level={} actual_level={:?} spill={} component={:?}",
                prior.site,
                prior.surface_elevation_m,
                geometry.water.level[prior.site],
                geometry.water.spill[prior.site].0,
                geometry.water.lake[prior.site]
            );
        }
    }

    #[test]
    #[ignore = "one actual canonical-scale Atlas to C2/current-water publication; Release only"]
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
        let artifact = AtlasTerrainGenerator::generate(&request, &cancellation).unwrap();
        let generate_ms = start.elapsed().as_millis();
        let snapshot = artifact.snapshot();
        let maximum_lake_edge = snapshot
            .support
            .faces
            .iter()
            .filter(|f| f.lake.is_some())
            .flat_map(|f| {
                (0..3).map(move |i| {
                    crate::world::spatial::central_angle(
                        snapshot.support.directions[f.nodes[i] as usize],
                        snapshot.support.directions[f.nodes[(i + 1) % 3] as usize],
                    )
                })
            })
            .fold(0.0, f64::max);
        assert!(maximum_lake_edge <= snapshot.fine.canonical_angular_step_radians());
        let audit_start = std::time::Instant::now();
        snapshot.validate().unwrap();
        let audit_ms = audit_start.elapsed().as_millis();
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
        eprintln!("ATLAS canonical total_ms={} nodes={} faces={} max_lake_edge_m={} lakes={} rivers={} initial_water_m3={} ocean_m3={} lake_m3={} budget={:?} climate={:?}",
            start.elapsed().as_millis(), snapshot.support.directions.len(), snapshot.support.faces.len(),
            maximum_lake_edge * snapshot.surface.radius().get(), snapshot.lakes.len(), snapshot.rivers.len(),
            snapshot.initial_water_inventory_m3, snapshot.fields.ocean_volume_m3.iter().sum::<f64>(),
            snapshot.fields.lake_volume_m3.iter().sum::<f64>(), snapshot.runoff_budget(), snapshot.climate().budget_report());
        report_coarse_bridge(snapshot, &cancellation);
        let suggestions = snapshot
            .climate_quality()
            .metrics()
            .iter()
            .filter(|metric| metric.status() != crate::world::natural::QualityMetricStatus::Pass)
            .map(|metric| (metric.id().name(), metric.status()))
            .collect::<Vec<_>>();
        eprintln!("ATLAS climate suggestions={suggestions:?}");
        report_mouth_connectivity(
            (
                snapshot.fine_terrain(),
                snapshot.surface(),
                &snapshot.support,
            ),
            &cancellation,
        );
        let publication = std::time::Instant::now();
        let mut stored = BuildArtifacts::default();
        stored.insert(artifact).unwrap();
        eprintln!(
            "ATLAS timing generate_ms={} explicit_full_audit_ms={} engine_validate_hash_ms={}",
            generate_ms,
            audit_ms,
            publication.elapsed().as_millis()
        );
    }

    fn report_coarse_bridge(snapshot: &AtlasTerrainSnapshot, cancellation: &BuildCancellation) {
        use crate::generators::spatial::remap_intensive_f32_cancellable;
        use crate::world::natural::SurfaceWaterKind;
        use crate::world::spatial::{central_angle, spherical_triangle_area_unit};
        let domain = super::super::ClimateWorkDomainBuilder::build(
            snapshot.surface(),
            NaturalQualityProfile::Draft,
            cancellation,
        )
        .unwrap();
        let remap = |values: &[f32]| {
            remap_intensive_f32_cancellable(domain.source_to_climate(), values, &|| false).unwrap()
        };
        let ocean = remap(snapshot.fields.ocean_fraction());
        let dry = remap(snapshot.fields.dry_fraction());
        let depth = remap(&climate::source_ocean_depth(
            snapshot.fields(),
            snapshot.surface(),
        ));
        let targets = [
            ("mouth", 560.0, 500.0),
            ("lake", 191.0, 680.0),
            ("seam", 0.0, 512.0),
        ];
        let target_ids = targets.map(|(_, x, y)| {
            let longitude = x / 2048.0 * std::f64::consts::TAU - std::f64::consts::PI;
            let latitude = std::f64::consts::FRAC_PI_2 - y / 1024.0 * std::f64::consts::PI;
            let point = UnitVector3::new(
                latitude.cos() * longitude.cos(),
                latitude.cos() * longitude.sin(),
                latitude.sin(),
            )
            .unwrap();
            domain
                .climate_surface()
                .cells()
                .iter()
                .max_by(|a, b| a.site.dot(point).total_cmp(&b.site.dot(point)))
                .unwrap()
                .id
                .raw() as usize
        });
        let (mut directions, triangles) = geometry::source_fans(domain.climate_surface());
        let mut triangles = triangles
            .into_iter()
            .filter(|(_, owner)| target_ids.contains(owner))
            .collect::<Vec<_>>();
        let spacing = snapshot.fine.canonical_angular_step_radians();
        while triangles.iter().any(|&(nodes, _)| {
            (0..3).any(|i| {
                central_angle(directions[nodes[i]], directions[nodes[(i + 1) % 3]]) > spacing
            })
        }) {
            triangles =
                geometry::refine_triangles(&mut directions, triangles, cancellation).unwrap();
        }
        let mut area = [0.0; 3];
        let mut ocean_area = [0.0; 3];
        let mut dry_area = [0.0; 3];
        let mut volume = [0.0; 3];
        for chunk in triangles.chunks(4096) {
            let query = chunk
                .iter()
                .map(|(nodes, _)| {
                    geometry::triangle_sample_direction(nodes.map(|i| directions[i])).components()
                })
                .collect::<Vec<_>>();
            let samples = snapshot.sample_surface_batch(&query, &|| false).unwrap();
            for ((nodes, owner), sample) in chunk.iter().zip(samples) {
                let index = target_ids.iter().position(|i| i == owner).unwrap();
                let [a, b, c] = nodes.map(|i| directions[i]);
                let weight =
                    spherical_triangle_area_unit(a, b, c) * snapshot.surface.radius().get().powi(2);
                area[index] += weight;
                match sample.water {
                    SurfaceWaterKind::Ocean => {
                        ocean_area[index] += weight;
                        volume[index] += weight * sample.water_depth_m().unwrap();
                    }
                    SurfaceWaterKind::DryLand => dry_area[index] += weight,
                    SurfaceWaterKind::Lake => {}
                }
            }
        }
        for i in 0..3 {
            eprintln!("ATLAS bridge window={} target={} direct_ocean={} remapped_ocean={} direct_dry={} remapped_dry={} direct_whole_depth_m={} remapped_whole_depth_m={}",
                targets[i].0,target_ids[i],ocean_area[i]/area[i],ocean[target_ids[i]],dry_area[i]/area[i],dry[target_ids[i]],volume[i]/area[i],depth[target_ids[i]]);
        }
    }

    fn report_mouth_connectivity(
        terrain: (
            &FineTerrain,
            &SphericalSurfaceSnapshot,
            &crate::world::natural::atlas_terrain::AtlasWaterSupport,
        ),
        cancellation: &BuildCancellation,
    ) {
        use crate::world::natural::SurfaceWaterKind;
        let (fine, surface, support) = terrain;
        let scale = usize::from(fine.scale().pixels_per_unit());
        let width = 32 * scale;
        let directions = (0..width * width)
            .map(|i| {
                let x = 544.75 + ((i % width) as f64 + 0.5) / scale as f64;
                let y = 484.0 + ((i / width) as f64 + 0.5) / scale as f64;
                let longitude = x / 2048.0 * std::f64::consts::TAU - std::f64::consts::PI;
                let latitude = std::f64::consts::FRAC_PI_2 - y / 1024.0 * std::f64::consts::PI;
                UnitVector3::new(
                    latitude.cos() * longitude.cos(),
                    latitude.cos() * longitude.sin(),
                    latitude.sin(),
                )
                .unwrap()
                .components()
            })
            .collect::<Vec<_>>();
        let samples = fine
            .sample_surface_batch(&directions, &|| false)
            .unwrap()
            .into_iter()
            .zip(&directions)
            .map(|(bed, &direction)| {
                crate::world::natural::atlas_terrain::bind_surface_sample(
                    fine,
                    surface,
                    support,
                    direction,
                    bed.bed_elevation_m,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let bed = samples
            .iter()
            .map(|sample| sample.bed_elevation_m)
            .collect::<Vec<_>>();
        let marine = samples
            .iter()
            .enumerate()
            .map(|(i, sample)| {
                (i % width == 0
                    || i % width == width - 1
                    || i / width == 0
                    || i / width == width - 1)
                    && sample.water == SurfaceWaterKind::Ocean
                    && sample.bed_elevation_m < 0.0
            })
            .collect::<Vec<_>>();
        let edges = (0..width * width).flat_map(|i| {
            [
                (i % width + 1 < width).then_some([i, i + 1]),
                (i / width + 1 < width).then_some([i, i + width]),
            ]
            .into_iter()
            .flatten()
        });
        let graph = geometry::SupportGraph::from_edges(samples.len(), edges, cancellation).unwrap();
        let water = initialize_water(&bed, &graph, &marine, &[], cancellation).unwrap();
        let false_negatives = samples
            .iter()
            .zip(&water.ocean)
            .filter(|(sample, ocean)| **ocean && sample.water == SurfaceWaterKind::DryLand)
            .count();
        eprintln!(
            "ATLAS mouth canonical connected-ocean false negatives={} samples={}",
            false_negatives,
            samples.len()
        );
        assert_eq!(
            false_negatives, 0,
            "known marine-connected fine pixels must not be clipped by a coarse support face"
        );
    }

    #[test]
    #[ignore = "actual terrain/water-only coastal visibility witness; no C2 rerun"]
    fn atlas_mouth_support_probe() {
        let _ = env_logger::Builder::new()
            .filter_module(
                "sekai::generators::natural::formation::atlas_terrain",
                log::LevelFilter::Debug,
            )
            .is_test(true)
            .try_init();
        let cancellation = BuildCancellation::new();
        let start = std::time::Instant::now();
        let (fine, surface) = prepare_source(
            &TerrainParams {
                seed: 42,
                ..Default::default()
            },
            FineScale::Four,
            &cancellation,
        )
        .unwrap();
        let geometry = geometry::build_geometry(&fine, &surface, 2, &cancellation).unwrap();
        let geometry =
            geometry::refine_water_support(&fine, &surface, geometry, &cancellation).unwrap();
        eprintln!(
            "ATLAS water support nodes={} faces={} total_ms={}",
            geometry.support.directions.len(),
            geometry.support.faces.len(),
            start.elapsed().as_millis()
        );
        report_mouth_connectivity((&fine, &surface, &geometry.support), &cancellation);
    }
}
