//! Immutable field document for the formation-product chain (P2v5→P5).
//!
//! Mirrors the natural-foundation document boundary: it extracts shared
//! artifacts from one complete `surface_formation_graph()` build outcome,
//! cross-validates their identities, and exposes the same renderer-independent
//! [`FieldDocument`] surface the spherical presenters already consume.

use std::sync::Arc;

use thiserror::Error;

use super::field_document::{owned_view_diagnostics, FieldDocument};
use super::natural_field_payloads::elevation_display_radius_m;
use crate::engine::{
    ArtifactError, BuildOutcome, BuildOutcomeIntegrityError, BuildProvenance, BuildReport,
    BuildResultHash,
};
use crate::generators::natural::{
    EvolvedTectonicArtifact, GlobalCirculationArtifact, NaturalSurfaceFormationArtifact,
    ResolvedTectonicInput, ResolvedTectonicInputArtifact,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::view::{
    DisplayRangeMode, FieldCatalog, FieldPayloadRef, FieldViewError, OwnedViewDiagnostic,
    SphericalPresentationSource,
};
use crate::world::fields::{FieldId, FieldRegistry, ValueRange};
use crate::world::natural::{
    annual_local_runoff_mm_field_id, circulation_annual_precipitation_mm_field_id,
    circulation_mean_air_temperature_c_field_id, circulation_prevailing_wind_m_s_field_id,
    coastal_deposition_m_field_id, coastal_erosion_m_field_id, crust_kind_field_id,
    crust_thickness_field_id, drainage_area_km2_field_id, fluvial_erosion_depth_m_field_id,
    formation_annual_precipitation_mm, hillslope_deposition_m_field_id,
    hillslope_erosion_m_field_id, isostatic_response_m_field_id, lake_depth_m_field_id,
    land_ocean_field_id, mean_annual_discharge_m3_s_field_id, plate_id_field_id,
    primary_elevation_m_field_id, routed_sediment_deposition_m_field_id,
    sediment_deposition_thickness_m_field_id, spherical_formation_field_registry,
    strahler_stream_order_field_id, surface_elevation_m_field_id, surface_water_kind_field_id,
    tectonic_displacement_m_field_id, GlobalCirculationFields, NaturalFieldRegistryError,
    SphericalTectonicValidationError, SurfaceFormationValidationError, CLIMATE_MONTH_COUNT,
};
use crate::world::spatial::{
    canonical_east_north_basis, SphericalSurfaceSnapshot, SphericalSurfaceValidationError,
    SurfaceRef,
};
use crate::world::RootSeed;

const SPHERICAL_FORMATION_GRAPH_CONTRACT_VERSION: u16 = 1;

/// Stable provenance identity for one complete formation-product document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SphericalFormationBuildIdentity {
    root_seed: RootSeed,
    surface_ref: SurfaceRef,
    build_result_hash: BuildResultHash,
    graph_contract_version: u16,
}

impl SphericalFormationBuildIdentity {
    fn new(provenance: &BuildProvenance, surface_ref: SurfaceRef) -> Self {
        Self {
            root_seed: provenance.root_seed(),
            surface_ref,
            build_result_hash: *provenance.result_hash(),
            graph_contract_version: SPHERICAL_FORMATION_GRAPH_CONTRACT_VERSION,
        }
    }

    /// Returns the root seed that drove the authoritative graph.
    pub(super) const fn root_seed(&self) -> RootSeed {
        self.root_seed
    }

    /// Returns the exact authoritative surface identity.
    pub(super) const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    /// Returns the engine-audited result hash of the complete build.
    pub(super) const fn build_result_hash(&self) -> &BuildResultHash {
        &self.build_result_hash
    }

    /// Returns the presentation contract version of the formation graph.
    pub(super) const fn graph_contract_version(&self) -> u16 {
        self.graph_contract_version
    }
}

/// Build-time authoring-compliance measurements for the formation product.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormationAreaSummary {
    authored_continental_fraction: f32,
    evolved_continental_fraction: f64,
    actual_land_fraction: f64,
    sea_level_m: f32,
}

impl FormationAreaSummary {
    /// Returns the author-requested initial continental crust area fraction.
    pub const fn authored_continental_fraction(&self) -> f32 {
        self.authored_continental_fraction
    }

    /// Returns the area-weighted evolved continental crust fraction.
    pub const fn evolved_continental_fraction(&self) -> f64 {
        self.evolved_continental_fraction
    }

    /// Returns the area-weighted land fraction of the published surface.
    pub const fn actual_land_fraction(&self) -> f64 {
        self.actual_land_fraction
    }

    /// Returns the water-volume-derived global sea level.
    pub const fn sea_level_m(&self) -> f32 {
        self.sea_level_m
    }
}

/// Owned display arrays derived once from the published circulation fields.
struct FormationDisplayCache {
    annual_precipitation_mm: Vec<f32>,
    mean_air_temperature_c: Vec<f32>,
    prevailing_wind_m_s: Vec<[f32; 2]>,
}

impl FormationDisplayCache {
    fn build(
        surface: &SphericalSurfaceSnapshot,
        fields: &GlobalCirculationFields,
    ) -> Result<Self, SphericalFormationDisplayError> {
        let cell_count = surface.cells().len();
        if fields.cell_count() != cell_count {
            return Err(SphericalFormationDisplayError::CirculationCardinality {
                circulation_cells: fields.cell_count(),
                surface_cells: cell_count,
            });
        }
        let monthly_precipitation = fields.monthly_precipitation_mm_day().values();
        let monthly_temperature = fields.monthly_air_temperature_c().values();
        let monthly_wind = fields.near_surface_wind_m_s().values();

        let mut annual_precipitation_mm = Vec::with_capacity(cell_count);
        let mut mean_air_temperature_c = Vec::with_capacity(cell_count);
        let mut prevailing_wind_m_s = Vec::with_capacity(cell_count);
        for (index, cell) in surface.cells().iter().enumerate() {
            annual_precipitation_mm.push(formation_annual_precipitation_mm(
                &monthly_precipitation[index],
            ));
            mean_air_temperature_c
                .push(monthly_temperature[index].iter().sum::<f32>() / CLIMATE_MONTH_COUNT as f32);
            let mut mean_wind = [0.0_f64; 3];
            for month in &monthly_wind[index] {
                for (axis, component) in month.iter().enumerate() {
                    mean_wind[axis] += f64::from(*component);
                }
            }
            for component in &mut mean_wind {
                *component /= CLIMATE_MONTH_COUNT as f64;
            }
            let (east, north) = canonical_east_north_basis(cell.centroid);
            let east_component =
                east[0] * mean_wind[0] + east[1] * mean_wind[1] + east[2] * mean_wind[2];
            let north_component =
                north[0] * mean_wind[0] + north[1] * mean_wind[1] + north[2] * mean_wind[2];
            prevailing_wind_m_s.push([east_component as f32, north_component as f32]);
        }
        Ok(Self {
            annual_precipitation_mm,
            mean_air_temperature_c,
            prevailing_wind_m_s,
        })
    }
}

/// Projection-free, immutable document for one complete formation world.
pub struct SphericalFormationFieldDocument {
    pub(super) surface: Arc<SphericalSurfaceArtifact>,
    resolved_tectonic: Arc<ResolvedTectonicInputArtifact>,
    tectonics: Arc<EvolvedTectonicArtifact>,
    circulation: Arc<GlobalCirculationArtifact>,
    pub(super) formation: Arc<NaturalSurfaceFormationArtifact>,
    registry: FieldRegistry,
    diagnostics: Vec<OwnedViewDiagnostic>,
    cache: FormationDisplayCache,
    area_summary: FormationAreaSummary,
    identity: SphericalFormationBuildIdentity,
}

impl SphericalFormationFieldDocument {
    /// Extracts shared artifacts and builds a fully cross-validated document.
    pub fn from_build_outcome(
        outcome: &BuildOutcome,
    ) -> Result<Self, SphericalFormationDisplayError> {
        let provenance = match outcome.verified_provenance() {
            Ok(provenance) => *provenance,
            Err(BuildOutcomeIntegrityError::MissingReportResultHash) => {
                return Err(SphericalFormationDisplayError::MissingBuildResultHash)
            }
            Err(error) => return Err(SphericalFormationDisplayError::BuildOutcomeIntegrity(error)),
        };
        Self::build(
            provenance,
            outcome.artifacts.get::<SphericalSurfaceArtifact>()?,
            outcome.artifacts.get::<ResolvedTectonicInputArtifact>()?,
            outcome.artifacts.get::<EvolvedTectonicArtifact>()?,
            outcome.artifacts.get::<GlobalCirculationArtifact>()?,
            outcome.artifacts.get::<NaturalSurfaceFormationArtifact>()?,
            &outcome.report,
        )
    }

    fn build(
        provenance: BuildProvenance,
        surface: Arc<SphericalSurfaceArtifact>,
        resolved_tectonic: Arc<ResolvedTectonicInputArtifact>,
        tectonics: Arc<EvolvedTectonicArtifact>,
        circulation: Arc<GlobalCirculationArtifact>,
        formation: Arc<NaturalSurfaceFormationArtifact>,
        report: &BuildReport,
    ) -> Result<Self, SphericalFormationDisplayError> {
        surface.snapshot().validate()?;
        let authoritative = SurfaceRef::for_spherical(surface.snapshot());
        tectonics
            .snapshot()
            .compatibility()
            .validate_against(surface.snapshot())?;
        let formation_snapshot = formation.snapshot();
        formation_snapshot.validate()?;
        if formation_snapshot.surface_ref() != authoritative {
            return Err(SphericalFormationDisplayError::FormationSurfaceMismatch {
                snapshot: formation_snapshot.surface_ref(),
                authoritative,
            });
        }

        let compatibility = tectonics.snapshot().compatibility();
        let plate_count = u16::try_from(compatibility.plates().len())
            .map_err(|_| SphericalFormationDisplayError::PlateCountOverflow)?;
        let registry = spherical_formation_field_registry(
            plate_count,
            surface.snapshot().total_cell_area().get(),
        )?;
        let cache =
            FormationDisplayCache::build(surface.snapshot(), circulation.snapshot().fields())?;

        let terrain = formation_snapshot.terrain_fields();
        let areas = surface.snapshot().cells();
        let total_area = surface.snapshot().total_cell_area().get();
        let crust_kinds = compatibility.crust_kinds().raw_values();
        let land = terrain.land_ocean().raw_values();
        let mut continental_area = 0.0_f64;
        let mut land_area = 0.0_f64;
        for (index, cell) in areas.iter().enumerate() {
            if crust_kinds[index] == 1 {
                continental_area += cell.area.get();
            }
            if land[index] == 1 {
                land_area += cell.area.get();
            }
        }
        let area_summary = FormationAreaSummary {
            authored_continental_fraction: resolved_tectonic
                .input()
                .spec()
                .continental_crust_fraction,
            evolved_continental_fraction: continental_area / total_area,
            actual_land_fraction: land_area / total_area,
            sea_level_m: terrain.sea_level_m(),
        };
        let identity = SphericalFormationBuildIdentity::new(&provenance, authoritative);
        let document = Self {
            surface,
            resolved_tectonic,
            tectonics,
            circulation,
            formation,
            registry,
            diagnostics: owned_view_diagnostics(report),
            cache,
            area_summary,
            identity,
        };
        document.catalog()?;
        Ok(document)
    }

    /// Returns the immutable audited identity of this document.
    pub(super) const fn identity(&self) -> &SphericalFormationBuildIdentity {
        &self.identity
    }

    /// Derives the presentation identity from this document's validated build identity.
    pub fn presentation_source(&self) -> SphericalPresentationSource {
        let identity = self.identity();
        SphericalPresentationSource::new(
            identity.root_seed(),
            identity.surface_ref(),
            *identity.build_result_hash(),
            identity.graph_contract_version(),
        )
    }

    /// Borrows the sole authoritative topology used by every presentation derivative.
    pub fn surface(&self) -> &crate::world::spatial::SphericalSurfaceSnapshot {
        self.surface.snapshot()
    }

    /// Borrows the resolved tectonic input that authored this world.
    pub fn resolved_tectonic_input(&self) -> &ResolvedTectonicInput {
        self.resolved_tectonic.input()
    }

    /// Borrows the published circulation snapshot backing the climate summaries.
    pub fn circulation(&self) -> &crate::world::natural::GlobalCirculationSnapshot {
        self.circulation.snapshot()
    }

    /// Borrows the validated catalog used to prepare fill and annotation layers.
    pub fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError> {
        <Self as FieldDocument>::catalog(self)
    }

    /// Borrows immutable document diagnostics used to prepare the shared mask.
    pub fn diagnostics(&self) -> &[OwnedViewDiagnostic] {
        &self.diagnostics
    }

    /// Borrows the build-time cached authoring-compliance measurements in O(1).
    pub const fn area_summary(&self) -> &FormationAreaSummary {
        &self.area_summary
    }

    /// Returns the product-preferred initial fill field.
    pub fn preferred_field(&self) -> Option<FieldId> {
        <Self as FieldDocument>::preferred_field(self)
    }

    /// Returns the document-authoritative preferred range for a field.
    pub fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode> {
        <Self as FieldDocument>::preferred_range(self, field)
    }
}

impl FieldDocument for SphericalFormationFieldDocument {
    fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError> {
        let compatibility = self.tectonics.snapshot().compatibility();
        let terrain = self.formation.snapshot().terrain_fields();
        let components = terrain.elevation_components();
        let hydrology = self.formation.snapshot().hydrology();
        let payloads: Vec<(FieldId, FieldPayloadRef<'_>)> = vec![
            (
                plate_id_field_id(),
                FieldPayloadRef::CategoryU32(compatibility.cell_plates().raw_values()),
            ),
            (
                crust_kind_field_id(),
                FieldPayloadRef::CategoryU32(compatibility.crust_kinds().raw_values()),
            ),
            (
                crust_thickness_field_id(),
                FieldPayloadRef::ScalarF32(compatibility.crust_thickness_km()),
            ),
            (
                primary_elevation_m_field_id(),
                FieldPayloadRef::ScalarF32(components.primary_elevation_m()),
            ),
            (
                tectonic_displacement_m_field_id(),
                FieldPayloadRef::ScalarF32(components.tectonic_displacement_m()),
            ),
            (
                fluvial_erosion_depth_m_field_id(),
                FieldPayloadRef::ScalarF32(components.fluvial_erosion_m()),
            ),
            (
                hillslope_erosion_m_field_id(),
                FieldPayloadRef::ScalarF32(components.hillslope_erosion_m()),
            ),
            (
                hillslope_deposition_m_field_id(),
                FieldPayloadRef::ScalarF32(components.hillslope_deposition_m()),
            ),
            (
                routed_sediment_deposition_m_field_id(),
                FieldPayloadRef::ScalarF32(components.routed_sediment_deposition_m()),
            ),
            (
                coastal_erosion_m_field_id(),
                FieldPayloadRef::ScalarF32(components.coastal_erosion_m()),
            ),
            (
                coastal_deposition_m_field_id(),
                FieldPayloadRef::ScalarF32(components.coastal_deposition_m()),
            ),
            (
                isostatic_response_m_field_id(),
                FieldPayloadRef::ScalarF32(components.isostatic_response_m()),
            ),
            (
                sediment_deposition_thickness_m_field_id(),
                FieldPayloadRef::ScalarF32(terrain.sediment().sediment_thickness_m()),
            ),
            (
                surface_elevation_m_field_id(),
                FieldPayloadRef::ScalarF32(terrain.final_elevation_m()),
            ),
            (
                land_ocean_field_id(),
                FieldPayloadRef::CategoryU32(terrain.land_ocean().raw_values()),
            ),
            (
                circulation_annual_precipitation_mm_field_id(),
                FieldPayloadRef::ScalarF32(&self.cache.annual_precipitation_mm),
            ),
            (
                circulation_mean_air_temperature_c_field_id(),
                FieldPayloadRef::ScalarF32(&self.cache.mean_air_temperature_c),
            ),
            (
                circulation_prevailing_wind_m_s_field_id(),
                FieldPayloadRef::Vector2F32(&self.cache.prevailing_wind_m_s),
            ),
            (
                annual_local_runoff_mm_field_id(),
                FieldPayloadRef::ScalarF32(hydrology.annual_local_runoff_mm()),
            ),
            (
                lake_depth_m_field_id(),
                FieldPayloadRef::ScalarF32(hydrology.lake_depth_m()),
            ),
            (
                surface_water_kind_field_id(),
                FieldPayloadRef::CategoryU32(hydrology.surface_water().raw_values()),
            ),
            (
                mean_annual_discharge_m3_s_field_id(),
                FieldPayloadRef::ScalarF32(hydrology.mean_annual_discharge_m3_s()),
            ),
            (
                drainage_area_km2_field_id(),
                FieldPayloadRef::ScalarF32(hydrology.drainage_area_km2()),
            ),
            (
                strahler_stream_order_field_id(),
                FieldPayloadRef::CategoryU32(hydrology.strahler_order().raw_values()),
            ),
        ];
        FieldCatalog::from_payloads(&self.registry, payloads)
    }

    fn diagnostics(&self) -> &[OwnedViewDiagnostic] {
        &self.diagnostics
    }

    fn preferred_field(&self) -> Option<FieldId> {
        Some(surface_elevation_m_field_id())
    }

    fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode> {
        formation_preferred_range(
            &self.registry,
            self.formation.snapshot().terrain_fields().sea_level_m(),
            self.formation
                .snapshot()
                .terrain_fields()
                .final_elevation_m(),
            field,
        )
    }
}

impl crate::app::field_document::SphericalFieldLayerDocument for SphericalFormationFieldDocument {
    fn presentation_source(&self) -> SphericalPresentationSource {
        self.presentation_source()
    }

    fn spherical_cell_count(&self) -> usize {
        self.surface.snapshot().cells().len()
    }

    fn spherical_edge_count(&self) -> usize {
        self.surface.snapshot().edges().len()
    }
}

/// Returns the shared formation-field range preference for the atomic surface.
fn formation_preferred_range(
    registry: &FieldRegistry,
    sea_level_m: f32,
    final_elevation_m: &[f32],
    field: &FieldId,
) -> Option<DisplayRangeMode> {
    if [
        annual_local_runoff_mm_field_id(),
        circulation_annual_precipitation_mm_field_id(),
        circulation_mean_air_temperature_c_field_id(),
        coastal_deposition_m_field_id(),
        coastal_erosion_m_field_id(),
        drainage_area_km2_field_id(),
        fluvial_erosion_depth_m_field_id(),
        hillslope_deposition_m_field_id(),
        hillslope_erosion_m_field_id(),
        isostatic_response_m_field_id(),
        lake_depth_m_field_id(),
        mean_annual_discharge_m3_s_field_id(),
        routed_sediment_deposition_m_field_id(),
        sediment_deposition_thickness_m_field_id(),
        tectonic_displacement_m_field_id(),
    ]
    .contains(field)
    {
        return Some(DisplayRangeMode::Data);
    }
    (field == &surface_elevation_m_field_id() || field == &primary_elevation_m_field_id())
        .then_some(())?;
    registry.get(field)?;
    let radius = elevation_display_radius_m(sea_level_m, final_elevation_m)?;
    ValueRange::new(sea_level_m - radius, sea_level_m + radius)
        .ok()
        .map(DisplayRangeMode::Manual)
}

/// Errors returned while composing a complete formation-product document.
#[derive(Debug, Error)]
pub enum SphericalFormationDisplayError {
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
    #[error(transparent)]
    BuildOutcomeIntegrity(BuildOutcomeIntegrityError),
    #[error(transparent)]
    Surface(#[from] SphericalSurfaceValidationError),
    #[error(transparent)]
    Tectonic(#[from] SphericalTectonicValidationError),
    #[error(transparent)]
    Formation(#[from] SurfaceFormationValidationError),
    #[error(transparent)]
    Registry(#[from] NaturalFieldRegistryError),
    #[error(transparent)]
    FieldView(#[from] FieldViewError),
    #[error("successful formation build report is missing its result hash")]
    MissingBuildResultHash,
    #[error("formation plate count cannot be represented by the field registry")]
    PlateCountOverflow,
    #[error(
        "published circulation covers {circulation_cells} cells but the surface has {surface_cells}"
    )]
    CirculationCardinality {
        circulation_cells: usize,
        surface_cells: usize,
    },
    #[error(
        "formation product {snapshot:?} does not match authoritative surface {authoritative:?}"
    )]
    FormationSurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
}
