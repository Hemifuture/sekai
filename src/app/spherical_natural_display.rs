use std::sync::Arc;

use thiserror::Error;

use super::field_document::{owned_view_diagnostics, FieldDocument};
use super::natural_field_payloads::{natural_preferred_range, NaturalFieldPayloadBundle};
use crate::engine::{ArtifactError, BuildOutcome, BuildReport, BuildResultHash};
use crate::generators::natural::{
    ResolvedWorldFormationArtifact, SphericalGeologicArtifact, SphericalHydroErosionArtifact,
    SphericalMantleArtifact, SphericalPreliminaryClimateArtifact, SphericalReliefArtifact,
    SphericalTectonicArtifact,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::view::{DisplayRangeMode, FieldCatalog, FieldViewError, OwnedViewDiagnostic};
use crate::world::fields::{FieldId, FieldRegistry};
use crate::world::natural::{
    spherical_natural_field_registry, surface_elevation_m_field_id, NaturalFieldRegistryError,
    SphericalClimateValidationError, SphericalGeologicValidationError,
    SphericalHydroErosionValidationError, SphericalMantleValidationError,
    SphericalReliefValidationError, SphericalTectonicValidationError, WorldFormationSpecError,
};
use crate::world::spatial::{
    canonical_east_north_basis, SphericalSurfaceValidationError, SurfaceRef, UnitVector3,
};
use crate::world::RootSeed;

const SPHERICAL_NATURAL_GRAPH_CONTRACT_VERSION: u16 = 1;

/// Stable provenance identity for one complete spherical natural document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SphericalNaturalBuildIdentity {
    root_seed: RootSeed,
    surface_ref: SurfaceRef,
    build_result_hash: BuildResultHash,
    graph_contract_version: u16,
}

impl SphericalNaturalBuildIdentity {
    fn new(
        root_seed: RootSeed,
        surface_ref: SurfaceRef,
        build_result_hash: BuildResultHash,
    ) -> Self {
        Self {
            root_seed,
            surface_ref,
            build_result_hash,
            graph_contract_version: SPHERICAL_NATURAL_GRAPH_CONTRACT_VERSION,
        }
    }

    /// Returns the root seed that drove the authoritative graph.
    pub(super) const fn root_seed(&self) -> RootSeed {
        self.root_seed
    }

    /// Returns the exact closed-surface identity shared by every snapshot.
    pub(super) const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    /// Returns the semantic hash of all successful graph outputs.
    pub(super) const fn build_result_hash(&self) -> &BuildResultHash {
        &self.build_result_hash
    }

    /// Returns the composition contract version used to interpret this identity.
    pub(super) const fn graph_contract_version(&self) -> u16 {
        self.graph_contract_version
    }
}

/// Rebuildable local tangent vectors and edge display values.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SphericalNaturalDisplayCache {
    plate_velocity_cm_per_year: Vec<[f32; 2]>,
    prevailing_wind_m_s: Vec<[f32; 2]>,
    boundary_kind: Vec<u32>,
    boundary_strength: Vec<f32>,
}

impl SphericalNaturalDisplayCache {
    fn build(
        surface: &SphericalSurfaceArtifact,
        tectonic: &SphericalTectonicArtifact,
        climate: &SphericalPreliminaryClimateArtifact,
    ) -> Result<Self, SphericalNaturalDisplayError> {
        let surface = surface.snapshot();
        let tectonic = tectonic.snapshot();
        let climate = climate.snapshot();
        let mut plate_velocity_cm_per_year = Vec::with_capacity(surface.cells().len());
        let mut prevailing_wind_m_s = Vec::with_capacity(surface.cells().len());

        for (index, cell) in surface.cells().iter().enumerate() {
            let plate = tectonic
                .plate_for_cell(cell.id)
                .expect("validated spherical tectonics cover every surface cell");
            let velocity_mm_per_year = tectonic.plates()[plate.raw() as usize]
                .rotation()
                .velocity_mm_per_year(surface.radius(), cell.centroid)?;
            let velocity = tangent_components(velocity_mm_per_year, cell.centroid, 0.1);
            plate_velocity_cm_per_year.push(velocity);

            let wind = climate.prevailing_wind_m_s()[index].map(f64::from);
            prevailing_wind_m_s.push(tangent_components(wind, cell.centroid, 1.0));
        }

        let boundary_kind = tectonic
            .boundaries()
            .iter()
            .map(|record| record.kind.raw())
            .collect();
        let boundary_strength = tectonic
            .boundaries()
            .iter()
            .map(|record| record.strength)
            .collect();
        Ok(Self {
            plate_velocity_cm_per_year,
            prevailing_wind_m_s,
            boundary_kind,
            boundary_strength,
        })
    }

    pub(super) fn plate_velocity_cm_per_year(&self) -> &[[f32; 2]] {
        &self.plate_velocity_cm_per_year
    }

    pub(super) fn prevailing_wind_m_s(&self) -> &[[f32; 2]] {
        &self.prevailing_wind_m_s
    }

    fn boundary_kind(&self) -> &[u32] {
        &self.boundary_kind
    }

    fn boundary_strength(&self) -> &[f32] {
        &self.boundary_strength
    }
}

fn tangent_components(vector: [f64; 3], radial: UnitVector3, scale: f64) -> [f32; 2] {
    let (east, north) = canonical_east_north_basis(radial);
    [
        (dot(vector, east) * scale) as f32,
        (dot(vector, north) * scale) as f32,
    ]
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

/// Projection-free, immutable document for one complete spherical natural world.
pub(super) struct SphericalNaturalFieldDocument {
    pub(super) surface: Arc<SphericalSurfaceArtifact>,
    pub(super) formation: Arc<ResolvedWorldFormationArtifact>,
    pub(super) tectonic: Arc<SphericalTectonicArtifact>,
    pub(super) mantle: Arc<SphericalMantleArtifact>,
    pub(super) relief: Arc<SphericalReliefArtifact>,
    pub(super) geology: Arc<SphericalGeologicArtifact>,
    pub(super) climate: Arc<SphericalPreliminaryClimateArtifact>,
    pub(super) hydro_erosion: Arc<SphericalHydroErosionArtifact>,
    registry: FieldRegistry,
    diagnostics: Vec<OwnedViewDiagnostic>,
    display_cache: SphericalNaturalDisplayCache,
    identity: SphericalNaturalBuildIdentity,
}

impl SphericalNaturalFieldDocument {
    /// Extracts shared Artifacts and builds a fully cross-validated document.
    pub(super) fn from_build_outcome(
        root_seed: RootSeed,
        outcome: &BuildOutcome,
    ) -> Result<Self, SphericalNaturalDisplayError> {
        Self::build(
            root_seed,
            outcome.artifacts.get::<SphericalSurfaceArtifact>()?,
            outcome.artifacts.get::<ResolvedWorldFormationArtifact>()?,
            outcome.artifacts.get::<SphericalTectonicArtifact>()?,
            outcome.artifacts.get::<SphericalMantleArtifact>()?,
            outcome.artifacts.get::<SphericalReliefArtifact>()?,
            outcome.artifacts.get::<SphericalGeologicArtifact>()?,
            outcome
                .artifacts
                .get::<SphericalPreliminaryClimateArtifact>()?,
            outcome.artifacts.get::<SphericalHydroErosionArtifact>()?,
            &outcome.report,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        root_seed: RootSeed,
        surface: Arc<SphericalSurfaceArtifact>,
        formation: Arc<ResolvedWorldFormationArtifact>,
        tectonic: Arc<SphericalTectonicArtifact>,
        mantle: Arc<SphericalMantleArtifact>,
        relief: Arc<SphericalReliefArtifact>,
        geology: Arc<SphericalGeologicArtifact>,
        climate: Arc<SphericalPreliminaryClimateArtifact>,
        hydro_erosion: Arc<SphericalHydroErosionArtifact>,
        report: &BuildReport,
    ) -> Result<Self, SphericalNaturalDisplayError> {
        let build_result_hash = *report
            .result_hash()
            .ok_or(SphericalNaturalDisplayError::MissingBuildResultHash)?;
        surface.snapshot().validate()?;
        formation.formation().validate()?;
        tectonic.snapshot().validate_against(surface.snapshot())?;
        mantle.snapshot().validate_against(surface.snapshot())?;
        relief.snapshot().validate_against(
            surface.snapshot(),
            tectonic.snapshot(),
            mantle.snapshot(),
        )?;
        geology.snapshot().validate_against(
            surface.snapshot(),
            tectonic.snapshot(),
            mantle.snapshot(),
            relief.snapshot(),
        )?;
        climate
            .snapshot()
            .validate_against(surface.snapshot(), relief.snapshot())?;
        hydro_erosion.snapshot().validate_against(
            surface.snapshot(),
            relief.snapshot(),
            geology.snapshot(),
            climate.snapshot(),
        )?;

        let plate_count = u16::try_from(tectonic.snapshot().plates().len())
            .map_err(|_| SphericalNaturalDisplayError::PlateCountOverflow)?;
        let registry = spherical_natural_field_registry(
            plate_count,
            surface.snapshot().total_cell_area().get(),
        )?;
        let display_cache = SphericalNaturalDisplayCache::build(&surface, &tectonic, &climate)?;
        let identity = SphericalNaturalBuildIdentity::new(
            root_seed,
            SurfaceRef::for_spherical(surface.snapshot()),
            build_result_hash,
        );
        let document = Self {
            surface,
            formation,
            tectonic,
            mantle,
            relief,
            geology,
            climate,
            hydro_erosion,
            registry,
            diagnostics: owned_view_diagnostics(report),
            display_cache,
            identity,
        };
        document.catalog()?;
        Ok(document)
    }

    /// Returns the immutable audited identity of this document.
    pub(super) const fn identity(&self) -> &SphericalNaturalBuildIdentity {
        &self.identity
    }
}

impl FieldDocument for SphericalNaturalFieldDocument {
    fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError> {
        let payloads = NaturalFieldPayloadBundle::from_spherical(
            self.tectonic.snapshot(),
            self.mantle.snapshot(),
            self.relief.snapshot(),
            self.geology.snapshot(),
            self.climate.snapshot(),
            self.hydro_erosion.snapshot(),
            self.display_cache.plate_velocity_cm_per_year(),
            self.display_cache.prevailing_wind_m_s(),
            self.display_cache.boundary_kind(),
            self.display_cache.boundary_strength(),
        );
        FieldCatalog::from_payloads(&self.registry, payloads.payloads())
    }

    fn diagnostics(&self) -> &[OwnedViewDiagnostic] {
        &self.diagnostics
    }

    fn preferred_field(&self) -> Option<FieldId> {
        Some(surface_elevation_m_field_id())
    }

    fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode> {
        natural_preferred_range(
            &self.registry,
            self.relief.snapshot().sea_level_m(),
            self.hydro_erosion
                .snapshot()
                .surface()
                .surface_elevation_m()
                .values(),
            field,
        )
    }
}

/// Atomically replaces a published document only after a candidate is complete.
pub(super) fn try_replace_spherical_natural_document(
    published: &mut Arc<SphericalNaturalFieldDocument>,
    root_seed: RootSeed,
    outcome: &BuildOutcome,
) -> Result<(), SphericalNaturalDisplayError> {
    let candidate = Arc::new(SphericalNaturalFieldDocument::from_build_outcome(
        root_seed, outcome,
    )?);
    *published = candidate;
    Ok(())
}

/// Errors returned while composing a complete spherical natural document.
#[derive(Debug, Error)]
pub(super) enum SphericalNaturalDisplayError {
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
    #[error(transparent)]
    Surface(#[from] SphericalSurfaceValidationError),
    #[error(transparent)]
    Formation(#[from] WorldFormationSpecError),
    #[error(transparent)]
    Tectonic(#[from] SphericalTectonicValidationError),
    #[error(transparent)]
    Mantle(#[from] SphericalMantleValidationError),
    #[error(transparent)]
    Relief(#[from] SphericalReliefValidationError),
    #[error(transparent)]
    Geologic(#[from] SphericalGeologicValidationError),
    #[error(transparent)]
    Climate(#[from] SphericalClimateValidationError),
    #[error(transparent)]
    HydroErosion(#[from] SphericalHydroErosionValidationError),
    #[error(transparent)]
    Registry(#[from] NaturalFieldRegistryError),
    #[error(transparent)]
    FieldView(#[from] FieldViewError),
    #[error("successful spherical natural build report is missing its result hash")]
    MissingBuildResultHash,
    #[error("spherical natural plate count cannot be represented by the field registry")]
    PlateCountOverflow,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        try_replace_spherical_natural_document, SphericalNaturalDisplayError,
        SphericalNaturalFieldDocument,
    };
    use crate::app::field_document::FieldDocument;
    use crate::engine::{
        BuildEngine, BuildOutcome, BuildReport, ExternalArtifacts, MemoryStageCache,
    };
    use crate::generators::natural::{
        spherical_natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact,
        GeologicSpecArtifact, HydroErosionSpecArtifact, ResolvedWorldFormationArtifact,
        RulePackSetArtifact, SphericalGeologicArtifact, SphericalHydroErosionArtifact,
        SphericalMantleArtifact, SphericalPreliminaryClimateArtifact, SphericalReliefArtifact,
        SphericalTectonicArtifact, TectonicSpecArtifact, WorldFormationSpecArtifact,
    };
    use crate::generators::spatial::{SphericalSpaceArtifact, SphericalSurfaceArtifact};
    use crate::rules::{default_rule_pack_set, AuthorConstraints};
    use crate::view::FieldCatalog;
    use crate::world::fields::{FieldDomain, FieldValueType};
    use crate::world::natural::{
        elevation_field_id, plate_id_field_id, plate_velocity_field_id,
        preliminary_mean_air_temperature_c_field_id, preliminary_prevailing_wind_m_s_field_id,
        surface_water_kind_field_id, ClimateSpec, GeologicSpec, HydroErosionSpec, TectonicSpec,
        WorldFormationSpec,
    };
    use crate::world::spatial::{canonical_east_north_basis, SurfaceRef};
    use crate::world::{Meters, RootSeed, SphericalSpaceSpec};

    const ROOT_SEED: RootSeed = RootSeed::new(42);
    const EXPECTED_FIELD_HASH: &str =
        "937bb06d57650e7f501fbc05fef9736a824aa41f7ce0f24d8b207cbe5afb7a66";

    fn build_outcome(radius_m: f64) -> BuildOutcome {
        let mut external = ExternalArtifacts::new();
        external
            .insert(SphericalSpaceArtifact::new(SphericalSpaceSpec {
                radius: Meters::new(radius_m).unwrap(),
                target_cell_count: 162,
            }))
            .unwrap();
        external
            .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
            .unwrap();
        external
            .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
            .unwrap();
        external
            .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
            .unwrap();
        external
            .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
            .unwrap();
        external
            .insert(WorldFormationSpecArtifact::new(
                WorldFormationSpec::default(),
            ))
            .unwrap();
        external
            .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
            .unwrap();
        external
            .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
            .unwrap();

        BuildEngine::new(spherical_natural_foundation_graph().unwrap())
            .build(ROOT_SEED, external, &mut MemoryStageCache::new())
            .unwrap()
    }

    fn assert_data_document<T: FieldDocument + ?Sized>(_document: &T) {}

    fn payload_catalog(document: &SphericalNaturalFieldDocument) -> FieldCatalog<'_> {
        document.catalog().unwrap()
    }

    fn hash_len(hasher: &mut blake3::Hasher, value: usize) {
        hasher.update(&(value as u64).to_le_bytes());
    }

    fn hash_text(hasher: &mut blake3::Hasher, value: &str) {
        hash_len(hasher, value.len());
        hasher.update(value.as_bytes());
    }

    fn field_hash(document: &SphericalNaturalFieldDocument) -> String {
        let catalog = payload_catalog(document);
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.spherical-natural-fields.v1\0");
        hash_len(&mut hasher, catalog.entries().len());
        for entry in catalog.entries() {
            let schema = entry.schema();
            hash_text(&mut hasher, schema.id.namespace());
            hash_text(&mut hasher, schema.id.name());
            hasher.update(&schema.id.version().to_le_bytes());
            hasher.update(&[match schema.domain {
                FieldDomain::Cells => 1,
                FieldDomain::Edges => 2,
                domain => panic!("unexpected spherical natural domain {domain:?}"),
            }]);
            let view = entry
                .view()
                .expect("the spherical document publishes every registered payload");
            match schema.value_type {
                FieldValueType::ScalarF32 => {
                    hasher.update(&[1]);
                    let values = view.scalar_values().unwrap();
                    hash_len(&mut hasher, values.len());
                    for value in values {
                        hasher.update(&value.to_bits().to_le_bytes());
                    }
                }
                FieldValueType::CategoryU32 => {
                    hasher.update(&[2]);
                    let values = view.category_values().unwrap();
                    hash_len(&mut hasher, values.len());
                    for value in values {
                        hasher.update(&value.to_le_bytes());
                    }
                }
                FieldValueType::Vector2F32 => {
                    hasher.update(&[3]);
                    let values = view.vector_values().unwrap();
                    hash_len(&mut hasher, values.len());
                    for value in values {
                        hasher.update(&value[0].to_bits().to_le_bytes());
                        hasher.update(&value[1].to_bits().to_le_bytes());
                    }
                }
                value_type => panic!("unexpected spherical natural payload {value_type:?}"),
            }
        }
        hasher.finalize().to_hex().to_string()
    }

    fn reconstruct(local: [f32; 2], radial: crate::world::spatial::UnitVector3) -> [f64; 3] {
        let (east, north) = canonical_east_north_basis(radial);
        std::array::from_fn(|axis| {
            east[axis] * f64::from(local[0]) + north[axis] * f64::from(local[1])
        })
    }

    struct TestArtifacts {
        surface: Arc<SphericalSurfaceArtifact>,
        formation: Arc<ResolvedWorldFormationArtifact>,
        tectonic: Arc<SphericalTectonicArtifact>,
        mantle: Arc<SphericalMantleArtifact>,
        relief: Arc<SphericalReliefArtifact>,
        geology: Arc<SphericalGeologicArtifact>,
        hydro_erosion: Arc<SphericalHydroErosionArtifact>,
    }

    fn get_artifacts(outcome: &BuildOutcome) -> TestArtifacts {
        TestArtifacts {
            surface: outcome.artifacts.get::<SphericalSurfaceArtifact>().unwrap(),
            formation: outcome
                .artifacts
                .get::<ResolvedWorldFormationArtifact>()
                .unwrap(),
            tectonic: outcome
                .artifacts
                .get::<SphericalTectonicArtifact>()
                .unwrap(),
            mantle: outcome.artifacts.get::<SphericalMantleArtifact>().unwrap(),
            relief: outcome.artifacts.get::<SphericalReliefArtifact>().unwrap(),
            geology: outcome
                .artifacts
                .get::<SphericalGeologicArtifact>()
                .unwrap(),
            hydro_erosion: outcome
                .artifacts
                .get::<SphericalHydroErosionArtifact>()
                .unwrap(),
        }
    }

    #[test]
    fn document_preserves_authoritative_build_identity_without_a_presenter() {
        let outcome = build_outcome(6_371_000.0);
        let document =
            SphericalNaturalFieldDocument::from_build_outcome(ROOT_SEED, &outcome).unwrap();

        assert_data_document(&document);
        assert_eq!(
            document.identity().surface_ref(),
            SurfaceRef::for_spherical(document.surface.snapshot())
        );
        assert_eq!(
            document.identity().build_result_hash(),
            outcome.report.result_hash().unwrap()
        );
        assert_eq!(document.identity().root_seed(), ROOT_SEED);
        assert_eq!(document.identity().graph_contract_version(), 1);
        assert_eq!(document.catalog().unwrap().entries().len(), 36);
    }

    #[test]
    fn document_publishes_every_payload_with_surface_cardinality_and_borrowed_storage() {
        let outcome = build_outcome(6_371_000.0);
        let document =
            SphericalNaturalFieldDocument::from_build_outcome(ROOT_SEED, &outcome).unwrap();
        let catalog = payload_catalog(&document);
        let cell_count = document.surface.snapshot().cells().len();
        let edge_count = document.surface.snapshot().edges().len();

        assert_eq!(catalog.entries().len(), 36);
        for entry in catalog.entries() {
            let expected_len = match entry.schema().domain {
                FieldDomain::Cells => cell_count,
                FieldDomain::Edges => edge_count,
                domain => panic!("unexpected spherical natural domain {domain:?}"),
            };
            assert_eq!(
                entry.view().unwrap().len(),
                expected_len,
                "wrong payload length for {:?}",
                entry.schema().id
            );
        }

        assert_eq!(
            catalog
                .get(&plate_id_field_id())
                .unwrap()
                .view()
                .unwrap()
                .category_values()
                .unwrap()
                .as_ptr(),
            document
                .tectonic
                .snapshot()
                .cell_plates()
                .raw_values()
                .as_ptr()
        );
        assert_eq!(
            catalog
                .get(&elevation_field_id())
                .unwrap()
                .view()
                .unwrap()
                .scalar_values()
                .unwrap()
                .as_ptr(),
            document.relief.snapshot().elevation_m().values().as_ptr()
        );
        assert_eq!(
            catalog
                .get(&preliminary_mean_air_temperature_c_field_id())
                .unwrap()
                .view()
                .unwrap()
                .scalar_values()
                .unwrap()
                .as_ptr(),
            document
                .climate
                .snapshot()
                .mean_annual_air_temperature_c()
                .as_ptr()
        );
        assert_eq!(
            catalog
                .get(&surface_water_kind_field_id())
                .unwrap()
                .view()
                .unwrap()
                .category_values()
                .unwrap()
                .as_ptr(),
            document
                .hydro_erosion
                .snapshot()
                .hydrology()
                .surface_water()
                .raw_values()
                .as_ptr()
        );
    }

    #[test]
    fn registry_order_field_bytes_are_deterministic_and_frozen() {
        let outcome = build_outcome(6_371_000.0);
        let first = SphericalNaturalFieldDocument::from_build_outcome(ROOT_SEED, &outcome).unwrap();
        let second =
            SphericalNaturalFieldDocument::from_build_outcome(ROOT_SEED, &outcome).unwrap();
        let first_hash = field_hash(&first);
        let second_hash = field_hash(&second);

        println!("spherical_natural_field_hash={first_hash}");
        assert_eq!(first_hash, second_hash);
        assert_eq!(first_hash, EXPECTED_FIELD_HASH);
    }

    #[test]
    fn local_east_north_vectors_reconstruct_authoritative_tangent_vectors() {
        let outcome = build_outcome(6_371_000.0);
        let document =
            SphericalNaturalFieldDocument::from_build_outcome(ROOT_SEED, &outcome).unwrap();
        let catalog = payload_catalog(&document);
        let local_plate_velocity = catalog
            .get(&plate_velocity_field_id())
            .unwrap()
            .view()
            .unwrap()
            .vector_values()
            .unwrap();
        let local_wind = catalog
            .get(&preliminary_prevailing_wind_m_s_field_id())
            .unwrap()
            .view()
            .unwrap()
            .vector_values()
            .unwrap();

        let mut maximum_plate_error_mm_per_year = 0.0_f64;
        let mut maximum_wind_error_m_s = 0.0_f64;
        for (index, cell) in document.surface.snapshot().cells().iter().enumerate() {
            let plate_id = document
                .tectonic
                .snapshot()
                .cell_plates()
                .get(index)
                .unwrap();
            let authoritative_plate = document.tectonic.snapshot().plates()
                [plate_id.raw() as usize]
                .rotation()
                .velocity_mm_per_year(document.surface.snapshot().radius(), cell.centroid)
                .unwrap();
            let reconstructed_plate = reconstruct(local_plate_velocity[index], cell.centroid)
                .map(|component| component * 10.0);
            for axis in 0..3 {
                maximum_plate_error_mm_per_year = maximum_plate_error_mm_per_year
                    .max((reconstructed_plate[axis] - authoritative_plate[axis]).abs());
            }

            let authoritative_wind = document.climate.snapshot().prevailing_wind_m_s()[index];
            let reconstructed_wind = reconstruct(local_wind[index], cell.centroid);
            for axis in 0..3 {
                maximum_wind_error_m_s = maximum_wind_error_m_s
                    .max((reconstructed_wind[axis] - f64::from(authoritative_wind[axis])).abs());
            }
        }
        assert!(maximum_plate_error_mm_per_year <= 1.0e-5);
        assert!(maximum_wind_error_m_s <= 1.0e-5);
    }

    #[test]
    fn equal_count_artifact_from_another_surface_is_rejected() {
        let first = build_outcome(6_371_000.0);
        let second = build_outcome(7_000_000.0);
        let artifacts = get_artifacts(&first);
        let foreign_climate = second
            .artifacts
            .get::<SphericalPreliminaryClimateArtifact>()
            .unwrap();
        assert_eq!(
            artifacts.surface.snapshot().cells().len(),
            second
                .artifacts
                .get::<SphericalSurfaceArtifact>()
                .unwrap()
                .snapshot()
                .cells()
                .len()
        );

        let result = SphericalNaturalFieldDocument::build(
            ROOT_SEED,
            artifacts.surface,
            artifacts.formation,
            artifacts.tectonic,
            artifacts.mantle,
            artifacts.relief,
            artifacts.geology,
            foreign_climate,
            artifacts.hydro_erosion,
            &first.report,
        );
        assert!(matches!(
            result,
            Err(SphericalNaturalDisplayError::Climate(_))
        ));
    }

    #[test]
    fn report_without_a_build_result_hash_is_rejected() {
        let mut outcome = build_outcome(6_371_000.0);
        outcome.report = BuildReport::new();

        assert!(matches!(
            SphericalNaturalFieldDocument::from_build_outcome(ROOT_SEED, &outcome),
            Err(SphericalNaturalDisplayError::MissingBuildResultHash)
        ));
    }

    #[test]
    fn rebuilding_the_document_reuses_artifacts_and_recreates_only_disposable_vectors() {
        let outcome = build_outcome(6_371_000.0);
        let first = SphericalNaturalFieldDocument::from_build_outcome(ROOT_SEED, &outcome).unwrap();
        let second =
            SphericalNaturalFieldDocument::from_build_outcome(ROOT_SEED, &outcome).unwrap();

        assert!(Arc::ptr_eq(&first.surface, &second.surface));
        assert!(Arc::ptr_eq(&first.formation, &second.formation));
        assert!(Arc::ptr_eq(&first.tectonic, &second.tectonic));
        assert!(Arc::ptr_eq(&first.mantle, &second.mantle));
        assert!(Arc::ptr_eq(&first.relief, &second.relief));
        assert!(Arc::ptr_eq(&first.geology, &second.geology));
        assert!(Arc::ptr_eq(&first.climate, &second.climate));
        assert!(Arc::ptr_eq(&first.hydro_erosion, &second.hydro_erosion));
        assert_eq!(first.display_cache, second.display_cache);
        assert_ne!(
            first.display_cache.plate_velocity_cm_per_year().as_ptr(),
            second.display_cache.plate_velocity_cm_per_year().as_ptr()
        );
        assert_ne!(
            first.display_cache.prevailing_wind_m_s().as_ptr(),
            second.display_cache.prevailing_wind_m_s().as_ptr()
        );
        assert_eq!(field_hash(&first), field_hash(&second));
    }

    #[test]
    fn failed_candidate_does_not_replace_the_published_document() {
        let valid = build_outcome(6_371_000.0);
        let mut published =
            Arc::new(SphericalNaturalFieldDocument::from_build_outcome(ROOT_SEED, &valid).unwrap());
        let before = Arc::clone(&published);
        let mut invalid = build_outcome(7_000_000.0);
        invalid.report = BuildReport::new();

        assert!(matches!(
            try_replace_spherical_natural_document(&mut published, ROOT_SEED, &invalid),
            Err(SphericalNaturalDisplayError::MissingBuildResultHash)
        ));
        assert!(Arc::ptr_eq(&published, &before));

        let replacement = build_outcome(7_000_000.0);
        try_replace_spherical_natural_document(&mut published, ROOT_SEED, &replacement).unwrap();
        assert!(!Arc::ptr_eq(&published, &before));
        assert_eq!(
            published.identity().surface_ref(),
            SurfaceRef::for_spherical(
                replacement
                    .artifacts
                    .get::<SphericalSurfaceArtifact>()
                    .unwrap()
                    .snapshot()
            )
        );
    }
}
