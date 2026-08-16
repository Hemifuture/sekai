use std::sync::Arc;

use thiserror::Error;

use super::field_document::{owned_view_diagnostics, FieldDocument};
use super::natural_field_payloads::{natural_preferred_range, NaturalFieldPayloadBundle};
use crate::engine::{
    ArtifactError, BuildOutcome, BuildOutcomeIntegrityError, BuildProvenance, BuildReport,
    BuildResultHash,
};
use crate::generators::natural::{
    ReliefSpecArtifact, ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact,
    SphericalGeologicArtifact, SphericalHydroErosionArtifact, SphericalMantleArtifact,
    SphericalPreliminaryClimateArtifact, SphericalReliefArtifact, SphericalTectonicArtifact,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::view::{
    DisplayRangeMode, FieldCatalog, FieldViewError, OwnedViewDiagnostic,
    SphericalPresentationSource,
};
use crate::world::fields::{FieldId, FieldRegistry};
use crate::world::natural::{
    spherical_natural_field_registry, surface_elevation_m_field_id, CrustKind,
    NaturalFieldRegistryError, SphericalClimateValidationError, SphericalGeologicValidationError,
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
    fn new(provenance: &BuildProvenance, surface_ref: SurfaceRef) -> Self {
        Self {
            root_seed: provenance.root_seed(),
            surface_ref,
            build_result_hash: *provenance.result_hash(),
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

/// Immutable author-target and authoritative-area measurements for one current world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SphericalNaturalAreaSummary {
    requested_initial_continental_crust_fraction: f64,
    evolved_continental_crust_fraction: f64,
    target_land_fraction: f64,
    actual_land_fraction: f64,
    sea_level_m: f32,
}

impl SphericalNaturalAreaSummary {
    fn build(
        surface: &SphericalSurfaceArtifact,
        resolved_tectonic: &ResolvedTectonicInputArtifact,
        tectonic: &SphericalTectonicArtifact,
        relief_spec: &ReliefSpecArtifact,
        relief: &SphericalReliefArtifact,
    ) -> Self {
        let total_area = surface.snapshot().total_cell_area().get();
        let evolved_continental_area = surface
            .snapshot()
            .cells()
            .iter()
            .zip(tectonic.snapshot().crust_kinds().raw_values())
            .filter_map(|(cell, &kind)| {
                (kind == CrustKind::Continental.raw()).then_some(cell.area.get())
            })
            .sum::<f64>();
        let actual_land_area = surface
            .snapshot()
            .cells()
            .iter()
            .zip(relief.snapshot().land_ocean().raw_values())
            .filter_map(|(cell, &kind)| (kind == 1).then_some(cell.area.get()))
            .sum::<f64>();
        Self {
            requested_initial_continental_crust_fraction: f64::from(
                resolved_tectonic.input().spec().continental_crust_fraction,
            ),
            evolved_continental_crust_fraction: evolved_continental_area / total_area,
            target_land_fraction: f64::from(relief_spec.spec().target_land_fraction),
            actual_land_fraction: actual_land_area / total_area,
            sea_level_m: relief.snapshot().sea_level_m(),
        }
    }

    /// Returns the resolved initial continental-crust share requested by the author.
    pub const fn requested_initial_continental_crust_fraction(self) -> f64 {
        self.requested_initial_continental_crust_fraction
    }

    /// Returns the current continental-crust share after bounded crust evolution.
    pub const fn evolved_continental_crust_fraction(self) -> f64 {
        self.evolved_continental_crust_fraction
    }

    /// Returns the authored target share of emergent land.
    pub const fn target_land_fraction(self) -> f64 {
        self.target_land_fraction
    }

    /// Returns the authoritative area share classified as land.
    pub const fn actual_land_fraction(self) -> f64 {
        self.actual_land_fraction
    }

    /// Returns the selected finite sea level in meters.
    pub const fn sea_level_m(self) -> f32 {
        self.sea_level_m
    }
}

/// Projection-free, immutable document for one complete spherical natural world.
pub struct SphericalNaturalFieldDocument {
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
    area_summary: SphericalNaturalAreaSummary,
    identity: SphericalNaturalBuildIdentity,
}

impl SphericalNaturalFieldDocument {
    /// Extracts shared Artifacts and builds a fully cross-validated document.
    pub(super) fn from_build_outcome(
        outcome: &BuildOutcome,
    ) -> Result<Self, SphericalNaturalDisplayError> {
        let provenance = match outcome.verified_provenance() {
            Ok(provenance) => *provenance,
            Err(BuildOutcomeIntegrityError::MissingReportResultHash) => {
                return Err(SphericalNaturalDisplayError::MissingBuildResultHash)
            }
            Err(error) => return Err(SphericalNaturalDisplayError::BuildOutcomeIntegrity(error)),
        };
        Self::build(
            provenance,
            outcome.artifacts.get::<SphericalSurfaceArtifact>()?,
            outcome.artifacts.get::<ResolvedWorldFormationArtifact>()?,
            outcome.artifacts.get::<ResolvedTectonicInputArtifact>()?,
            outcome.artifacts.get::<SphericalTectonicArtifact>()?,
            outcome.artifacts.get::<SphericalMantleArtifact>()?,
            outcome.artifacts.get::<SphericalReliefArtifact>()?,
            outcome.artifacts.get::<ReliefSpecArtifact>()?,
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
        provenance: BuildProvenance,
        surface: Arc<SphericalSurfaceArtifact>,
        formation: Arc<ResolvedWorldFormationArtifact>,
        resolved_tectonic: Arc<ResolvedTectonicInputArtifact>,
        tectonic: Arc<SphericalTectonicArtifact>,
        mantle: Arc<SphericalMantleArtifact>,
        relief: Arc<SphericalReliefArtifact>,
        relief_spec: Arc<ReliefSpecArtifact>,
        geology: Arc<SphericalGeologicArtifact>,
        climate: Arc<SphericalPreliminaryClimateArtifact>,
        hydro_erosion: Arc<SphericalHydroErosionArtifact>,
        report: &BuildReport,
    ) -> Result<Self, SphericalNaturalDisplayError> {
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
        let area_summary = SphericalNaturalAreaSummary::build(
            &surface,
            &resolved_tectonic,
            &tectonic,
            &relief_spec,
            &relief,
        );
        let identity = SphericalNaturalBuildIdentity::new(
            &provenance,
            SurfaceRef::for_spherical(surface.snapshot()),
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
            area_summary,
            identity,
        };
        document.catalog()?;
        Ok(document)
    }

    /// Returns the immutable audited identity of this document.
    pub(super) const fn identity(&self) -> &SphericalNaturalBuildIdentity {
        &self.identity
    }

    /// Derives the presentation identity from this document's validated natural-build identity.
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

    /// Borrows the validated catalog used to prepare fill and annotation layers.
    pub fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError> {
        <Self as FieldDocument>::catalog(self)
    }

    /// Borrows immutable document diagnostics used to prepare the shared mask.
    pub fn diagnostics(&self) -> &[OwnedViewDiagnostic] {
        &self.diagnostics
    }

    /// Borrows the build-time cached authoring-compliance measurements in O(1).
    pub const fn area_summary(&self) -> &SphericalNaturalAreaSummary {
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

    /// Borrows the validated field catalog for crate-internal product UI.
    pub(crate) fn catalog_for_ui(&self) -> Result<FieldCatalog<'_>, FieldViewError> {
        self.catalog()
    }

    /// Borrows document-owned diagnostics for crate-internal product UI.
    pub(crate) fn diagnostics_for_ui(&self) -> &[OwnedViewDiagnostic] {
        self.diagnostics()
    }

    /// Borrows the sole authoritative spherical topology for crate-internal product UI.
    pub(crate) fn surface_for_ui(&self) -> &crate::world::spatial::SphericalSurfaceSnapshot {
        self.surface()
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

impl crate::app::field_document::SphericalFieldLayerDocument for SphericalNaturalFieldDocument {
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

/// Atomically replaces a published document only after a candidate is complete.
pub(super) fn try_replace_spherical_natural_document(
    published: &mut Arc<SphericalNaturalFieldDocument>,
    outcome: &BuildOutcome,
) -> Result<(), SphericalNaturalDisplayError> {
    let candidate = Arc::new(SphericalNaturalFieldDocument::from_build_outcome(outcome)?);
    *published = candidate;
    Ok(())
}

/// Errors returned while composing a complete spherical natural document.
#[derive(Debug, Error)]
pub enum SphericalNaturalDisplayError {
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
    #[error(transparent)]
    BuildOutcomeIntegrity(BuildOutcomeIntegrityError),
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
    use std::cell::Cell;
    use std::sync::Arc;

    use eframe::egui_wgpu::wgpu;

    use super::{
        try_replace_spherical_natural_document, SphericalNaturalDisplayError,
        SphericalNaturalFieldDocument,
    };
    use crate::app::field_document::{
        prepare_spherical_document_layers, reconcile_spherical_document_camera,
        update_spherical_document_layers, FieldDocument, SphericalFieldLayerDocument,
    };
    use crate::engine::{
        BuildEngine, BuildOutcome, BuildOutcomeIntegrityError, BuildReport, ExternalArtifacts,
        MemoryStageCache,
    };
    use crate::generators::natural::{
        spherical_natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact,
        GeologicSpecArtifact, HydroErosionSpecArtifact, ReliefSpecArtifact,
        ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact, RulePackSetArtifact,
        SphericalGeologicArtifact, SphericalHydroErosionArtifact, SphericalMantleArtifact,
        SphericalPreliminaryClimateArtifact, SphericalReliefArtifact, SphericalTectonicArtifact,
        TectonicSpecArtifact, WorldFormationSpecArtifact,
    };
    use crate::generators::spatial::{SphericalSpaceArtifact, SphericalSurfaceArtifact};
    use crate::gpu::spherical::{
        installed_overlay_arc_ids, validation_probe, SphericalFieldRenderer, SphericalGpuPacket,
    };
    use crate::rules::{default_rule_pack_set, AuthorConstraints};
    use crate::view::{
        built_in_palette, classify_spherical_channel, field_layer_preparation_counts,
        prepare_edge_field, prepare_spherical_field_layers, reset_field_layer_preparation_counts,
        DisplayPrepareError, DisplayRangeMode, DisplayRevision, DisplayRevisionClock, FieldCatalog,
        GlobeCamera, GlyphLodKey, MapCamera, OwnedViewDiagnostic, PaletteId, PreparedFieldLayers,
        PreparedGlobeMesh, PreparedOverlayKind, PreparedProjectedMap, PreparedSphericalOverlay,
        SphericalFieldChannel, SphericalFieldDisplayState, SphericalMeshBudgets,
        SphericalPresentationSource, SphericalProjection, SphericalProjectionKind,
        SphericalViewMode, VectorGlyphLod, ViewDiagnosticSeverity,
    };
    use crate::world::fields::{FieldDomain, FieldValueType};
    use crate::world::natural::{
        boundary_kind_field_id, boundary_strength_field_id, crust_thickness_field_id,
        elevation_field_id, plate_id_field_id, plate_velocity_field_id,
        preliminary_mean_air_temperature_c_field_id, preliminary_prevailing_wind_m_s_field_id,
        surface_elevation_m_field_id, surface_water_kind_field_id, ClimateSpec, CrustKind,
        GeologicSpec, HydroErosionSpec, ReliefSpec, TectonicSpec, WorldFormationSpec,
    };
    use crate::world::spatial::{canonical_east_north_basis, SurfaceRef};
    use crate::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

    const ROOT_SEED: RootSeed = RootSeed::new(42);
    const EXPECTED_FIELD_HASH: &str =
        "6459f8178bb3c34531a5f9139c1fa59b69591af5e61ee0f1ce15ab0aa22c5d54";

    struct CountingSphericalLayerDocument<'a> {
        inner: &'a SphericalNaturalFieldDocument,
        catalog_calls: Cell<usize>,
    }

    impl<'a> CountingSphericalLayerDocument<'a> {
        fn new(inner: &'a SphericalNaturalFieldDocument) -> Self {
            Self {
                inner,
                catalog_calls: Cell::new(0),
            }
        }

        fn reset_catalog_calls(&self) {
            self.catalog_calls.set(0);
        }

        fn catalog_calls(&self) -> usize {
            self.catalog_calls.get()
        }
    }

    impl FieldDocument for CountingSphericalLayerDocument<'_> {
        fn catalog(&self) -> Result<FieldCatalog<'_>, crate::view::FieldViewError> {
            self.catalog_calls.set(self.catalog_calls.get() + 1);
            self.inner.catalog()
        }

        fn diagnostics(&self) -> &[OwnedViewDiagnostic] {
            self.inner.diagnostics()
        }

        fn preferred_field(&self) -> Option<crate::world::fields::FieldId> {
            self.inner.preferred_field()
        }

        fn preferred_range(
            &self,
            field: &crate::world::fields::FieldId,
        ) -> Option<DisplayRangeMode> {
            self.inner.preferred_range(field)
        }
    }

    impl SphericalFieldLayerDocument for CountingSphericalLayerDocument<'_> {
        fn presentation_source(&self) -> SphericalPresentationSource {
            self.inner.presentation_source()
        }

        fn spherical_cell_count(&self) -> usize {
            self.inner.surface.snapshot().cells().len()
        }

        fn spherical_edge_count(&self) -> usize {
            self.inner.surface.snapshot().edges().len()
        }
    }

    fn build_outcome_with_specs(
        root_seed: RootSeed,
        radius_m: f64,
        tectonic_spec: TectonicSpec,
        relief_spec: ReliefSpec,
    ) -> BuildOutcome {
        let mut external = ExternalArtifacts::new();
        external
            .insert(SphericalSpaceArtifact::new(SphericalSpaceSpec {
                radius: Meters::new(radius_m).unwrap(),
                target_cell_count: 162,
            }))
            .unwrap();
        external
            .insert(TectonicSpecArtifact::new(tectonic_spec))
            .unwrap();
        external
            .insert(ReliefSpecArtifact::new(relief_spec))
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
            .build(root_seed, external, &mut MemoryStageCache::new())
            .unwrap()
    }

    fn build_outcome_with_seed(root_seed: RootSeed, radius_m: f64) -> BuildOutcome {
        build_outcome_with_specs(
            root_seed,
            radius_m,
            TectonicSpec::default(),
            ReliefSpec::default(),
        )
    }

    fn build_outcome(radius_m: f64) -> BuildOutcome {
        build_outcome_with_seed(ROOT_SEED, radius_m)
    }

    fn assert_data_document<T: FieldDocument + ?Sized>(_document: &T) {}

    fn payload_catalog(document: &SphericalNaturalFieldDocument) -> FieldCatalog<'_> {
        document.catalog().unwrap()
    }

    fn next_revision(clock: &DisplayRevisionClock) -> DisplayRevision {
        let mut probe = clock.clone();
        probe.issue().unwrap()
    }

    fn assert_only_vector_glyph_revision_changed(
        before: &PreparedFieldLayers,
        after: &PreparedFieldLayers,
    ) {
        assert_eq!(before.revisions().fill, after.revisions().fill);
        assert_eq!(before.revisions().overlay, after.revisions().overlay);
        assert_eq!(
            before.revisions().diagnostics,
            after.revisions().diagnostics
        );
        assert_eq!(
            before.revisions().fill_palette,
            after.revisions().fill_palette
        );
        assert_eq!(
            before.revisions().overlay_palette,
            after.revisions().overlay_palette
        );
        assert_ne!(
            before.revisions().vector_glyphs,
            after.revisions().vector_glyphs
        );
    }

    fn request_spherical_test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: true,
                    compatible_surface: None,
                })
                .await;
            let adapter = match adapter {
                Some(adapter) => adapter,
                None => {
                    let adapter = instance
                        .request_adapter(&wgpu::RequestAdapterOptions {
                            power_preference: wgpu::PowerPreference::LowPower,
                            force_fallback_adapter: false,
                            compatible_surface: None,
                        })
                        .await;
                    let Some(adapter) = adapter else {
                        return gpu_unavailable("no fallback or hardware adapter is available");
                    };
                    adapter
                }
            };
            match adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("Spherical Camera Reconciliation Test Device"),
                        required_limits: wgpu::Limits::downlevel_defaults(),
                        ..Default::default()
                    },
                    None,
                )
                .await
            {
                Ok(device) => Some(device),
                Err(error) => gpu_unavailable(&format!("test device request failed: {error}")),
            }
        })
    }

    fn gpu_unavailable<T>(reason: &str) -> Option<T> {
        if std::env::var("SEKAI_REQUIRE_SPHERICAL_GPU").as_deref() == Ok("1") {
            panic!("spherical GPU evidence is required: {reason}");
        }
        eprintln!("skipping optional spherical GPU test: {reason}");
        None
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
        resolved_tectonic: Arc<ResolvedTectonicInputArtifact>,
        tectonic: Arc<SphericalTectonicArtifact>,
        mantle: Arc<SphericalMantleArtifact>,
        relief: Arc<SphericalReliefArtifact>,
        relief_spec: Arc<ReliefSpecArtifact>,
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
            resolved_tectonic: outcome
                .artifacts
                .get::<ResolvedTectonicInputArtifact>()
                .unwrap(),
            tectonic: outcome
                .artifacts
                .get::<SphericalTectonicArtifact>()
                .unwrap(),
            mantle: outcome.artifacts.get::<SphericalMantleArtifact>().unwrap(),
            relief: outcome.artifacts.get::<SphericalReliefArtifact>().unwrap(),
            relief_spec: outcome.artifacts.get::<ReliefSpecArtifact>().unwrap(),
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
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();

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
    fn document_caches_authoritative_area_compliance() {
        fn independently_measure(outcome: &BuildOutcome) -> (f64, f64) {
            let surface = outcome.artifacts.get::<SphericalSurfaceArtifact>().unwrap();
            let tectonic = outcome
                .artifacts
                .get::<SphericalTectonicArtifact>()
                .unwrap();
            let relief = outcome.artifacts.get::<SphericalReliefArtifact>().unwrap();
            let total_area = surface.snapshot().total_cell_area().get();
            let evolved_continental_area = surface
                .snapshot()
                .cells()
                .iter()
                .zip(tectonic.snapshot().crust_kinds().raw_values())
                .filter_map(|(cell, &kind)| {
                    (kind == CrustKind::Continental.raw()).then_some(cell.area.get())
                })
                .sum::<f64>();
            let actual_land_area = surface
                .snapshot()
                .cells()
                .iter()
                .zip(relief.snapshot().land_ocean().raw_values())
                .filter_map(|(cell, &kind)| (kind == 1).then_some(cell.area.get()))
                .sum::<f64>();
            (
                evolved_continental_area / total_area,
                actual_land_area / total_area,
            )
        }

        let tectonic_spec = TectonicSpec {
            continental_crust_fraction: 0.44,
            ..TectonicSpec::default()
        };
        let relief_spec = ReliefSpec {
            target_land_fraction: 0.57,
            ..ReliefSpec::default()
        };
        let outcome =
            build_outcome_with_specs(ROOT_SEED, 6_371_000.0, tectonic_spec, relief_spec.clone());
        let resolved = outcome
            .artifacts
            .get::<ResolvedTectonicInputArtifact>()
            .unwrap();
        let (expected_evolved, expected_land) = independently_measure(&outcome);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let summary = document.area_summary();

        assert_eq!(
            summary.requested_initial_continental_crust_fraction(),
            f64::from(resolved.input().spec().continental_crust_fraction)
        );
        assert_eq!(
            summary.evolved_continental_crust_fraction(),
            expected_evolved
        );
        assert_eq!(
            summary.target_land_fraction(),
            f64::from(relief_spec.target_land_fraction)
        );
        assert_eq!(summary.actual_land_fraction(), expected_land);
        assert_eq!(
            summary.sea_level_m(),
            outcome
                .artifacts
                .get::<SphericalReliefArtifact>()
                .unwrap()
                .snapshot()
                .sea_level_m()
        );

        let summary_address = std::ptr::from_ref(summary);
        let preparation_before = field_layer_preparation_counts();
        for _ in 0..20_000 {
            assert_eq!(summary_address, std::ptr::from_ref(document.area_summary()));
        }
        assert_eq!(field_layer_preparation_counts(), preparation_before);

        let replacement = build_outcome_with_specs(
            RootSeed::new(43),
            6_371_000.0,
            TectonicSpec::default(),
            ReliefSpec {
                target_land_fraction: 0.25,
                ..ReliefSpec::default()
            },
        );
        let mut published = Arc::new(document);
        try_replace_spherical_natural_document(&mut published, &replacement).unwrap();
        assert_ne!(
            summary_address,
            std::ptr::from_ref(published.area_summary())
        );
        assert_eq!(published.area_summary().target_land_fraction(), 0.25);
    }

    #[test]
    fn presentation_source_derives_every_value_from_the_validated_document_identity() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let source: SphericalPresentationSource = document.presentation_source();

        assert_eq!(source.root_seed(), document.identity().root_seed());
        assert_eq!(source.surface_ref(), document.identity().surface_ref());
        assert_eq!(
            source.build_result_hash(),
            document.identity().build_result_hash()
        );
        assert_eq!(
            source.graph_contract_version(),
            document.identity().graph_contract_version()
        );
    }

    #[test]
    fn complete_spherical_catalog_prepares_fill_edge_and_vector_layers() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let catalog = payload_catalog(&document);
        let counts = catalog
            .entries()
            .iter()
            .fold([0usize; 3], |mut counts, entry| {
                let schema = entry.schema();
                match classify_spherical_channel(schema.domain, schema.value_type).unwrap() {
                    SphericalFieldChannel::CellFill => counts[0] += 1,
                    SphericalFieldChannel::EdgeOverlay => counts[1] += 1,
                    SphericalFieldChannel::VectorOverlay => counts[2] += 1,
                }
                counts
            });
        assert_eq!(counts, [32, 2, 2]);
        assert_eq!(catalog.entries().len(), 36);

        let mut state = SphericalFieldDisplayState::default();
        state.select_fill(surface_elevation_m_field_id());
        state.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
        let mut clock = crate::view::DisplayRevisionClock::default();
        let layers = prepare_spherical_field_layers(
            document.presentation_source(),
            &catalog,
            document.surface.snapshot().cells().len(),
            document.surface.snapshot().edges().len(),
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();

        let PreparedSphericalOverlay::Vector(vector) = layers.overlay().unwrap() else {
            panic!("wind must prepare as a vector overlay");
        };
        let original = catalog
            .get(&preliminary_prevailing_wind_m_s_field_id())
            .unwrap()
            .view()
            .unwrap()
            .vector_values()
            .unwrap();
        assert_eq!(vector.components(), original);
        assert!(vector.magnitudes().iter().all(|value| value.is_finite()));
        assert!(vector.display_range().bounds().0 <= vector.display_range().bounds().1);
        assert_eq!(layers.overlay_kind(), Some(PreparedOverlayKind::CellVector));
        assert_eq!(
            layers.overlay_palette().unwrap(),
            built_in_palette(PaletteId::Sequential)
        );

        state.select_overlay(Some(boundary_kind_field_id()));
        let category = prepare_spherical_field_layers(
            document.presentation_source(),
            &catalog,
            document.surface.snapshot().cells().len(),
            document.surface.snapshot().edges().len(),
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();
        let PreparedSphericalOverlay::Edge(edge) = category.overlay().unwrap() else {
            panic!("boundary kind must prepare as an edge overlay");
        };
        assert_eq!(edge.len(), document.surface.snapshot().edges().len());
        assert_eq!(edge.kind(), crate::view::PreparedFieldKind::Category);
        assert!(!edge.category_keys().is_empty());
        assert_eq!(
            category.overlay_kind(),
            Some(PreparedOverlayKind::EdgeCategory)
        );

        state.select_overlay(Some(boundary_strength_field_id()));
        let scalar = prepare_spherical_field_layers(
            document.presentation_source(),
            &catalog,
            document.surface.snapshot().cells().len(),
            document.surface.snapshot().edges().len(),
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();
        let PreparedSphericalOverlay::Edge(edge) = scalar.overlay().unwrap() else {
            panic!("boundary strength must prepare as an edge overlay");
        };
        assert_eq!(edge.kind(), crate::view::PreparedFieldKind::Scalar);
        assert!(edge.display_range().is_some());
        assert_eq!(scalar.overlay_kind(), Some(PreparedOverlayKind::EdgeScalar));

        let shared = std::sync::Arc::new(layers);
        let map_layers = shared.clone();
        let globe_layers = shared.clone();
        assert!(std::sync::Arc::ptr_eq(&map_layers, &globe_layers));
        assert!(std::sync::Arc::ptr_eq(
            map_layers.fill_arc(),
            globe_layers.fill_arc()
        ));
        assert!(std::sync::Arc::ptr_eq(
            map_layers.fill_palette_arc(),
            globe_layers.fill_palette_arc()
        ));
        assert!(std::sync::Arc::ptr_eq(
            map_layers.diagnostics_arc(),
            globe_layers.diagnostics_arc()
        ));
    }

    #[test]
    fn spherical_layer_updates_replace_only_changed_payloads() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let catalog = payload_catalog(&document);
        let cell_count = document.surface.snapshot().cells().len();
        let edge_count = document.surface.snapshot().edges().len();
        let mut state = SphericalFieldDisplayState::default();
        state.select_fill(surface_elevation_m_field_id());
        state.select_overlay(Some(boundary_kind_field_id()));
        let mut clock = crate::view::DisplayRevisionClock::default();
        let initial = prepare_spherical_field_layers(
            document.presentation_source(),
            &catalog,
            cell_count,
            edge_count,
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();

        state.set_diagnostics_enabled(false);
        let toggled = crate::view::update_spherical_field_layers(
            &initial,
            document.presentation_source(),
            &catalog,
            cell_count,
            edge_count,
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert!(!toggled.diagnostics_enabled());
        assert!(std::sync::Arc::ptr_eq(
            initial.fill_arc(),
            toggled.fill_arc()
        ));
        assert!(std::sync::Arc::ptr_eq(
            initial.diagnostics_arc(),
            toggled.diagnostics_arc()
        ));
        assert_eq!(initial.revisions(), toggled.revisions());

        state.select_fill(elevation_field_id());
        let changed_fill = crate::view::update_spherical_field_layers(
            &toggled,
            document.presentation_source(),
            &catalog,
            cell_count,
            edge_count,
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert!(!std::sync::Arc::ptr_eq(
            toggled.fill_arc(),
            changed_fill.fill_arc()
        ));
        let (PreparedSphericalOverlay::Edge(before), PreparedSphericalOverlay::Edge(after)) =
            (toggled.overlay().unwrap(), changed_fill.overlay().unwrap())
        else {
            panic!("the unchanged boundary overlay must remain an edge field");
        };
        assert!(std::sync::Arc::ptr_eq(before, after));
        assert_eq!(
            toggled.revisions().overlay,
            changed_fill.revisions().overlay
        );
        assert_ne!(toggled.revisions().fill, changed_fill.revisions().fill);
    }

    #[test]
    fn spherical_document_binds_layer_preparation_to_its_own_source_and_cardinality() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let mut state = SphericalFieldDisplayState::default();
        let mut clock = crate::view::DisplayRevisionClock::default();

        let layers = prepare_spherical_document_layers(
            &document,
            SphericalViewMode::Map,
            SphericalProjectionKind::EqualEarth,
            MapCamera::default(),
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();

        assert_eq!(layers.source(), &document.presentation_source());
        assert_eq!(
            layers.fill().len(),
            document.surface.snapshot().cells().len()
        );
    }

    #[test]
    fn camera_only_document_reconciliation_retains_the_outer_arc_and_skips_scans_in_band() {
        let outcome = build_outcome(6_371_000.0);
        let mut document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        document.diagnostics.push(OwnedViewDiagnostic {
            severity: ViewDiagnosticSeverity::Warning,
            code: "test.camera-lod-diagnostic".into(),
            field_id: Some(plate_velocity_field_id()),
            cell_id: Some(CellId::from_raw(0)),
            message: "nonempty diagnostic fingerprint fixture".into(),
        });
        let expected_diagnostic_scans = document.diagnostics.len();
        let document = CountingSphericalLayerDocument::new(&document);
        let mut state = SphericalFieldDisplayState::default();
        state.select_overlay(Some(plate_velocity_field_id()));
        state.set_vector_lod(VectorGlyphLod::Low);
        let mut clock = DisplayRevisionClock::default();
        let projection = SphericalProjectionKind::EqualEarth;
        let mut map_camera = MapCamera::default();
        assert!(map_camera.zoom_by(projection, 1.99));
        let low = Arc::new(
            prepare_spherical_document_layers(
                &document,
                SphericalViewMode::Map,
                projection,
                map_camera,
                GlobeCamera::default(),
                &mut state,
                &mut clock,
            )
            .unwrap(),
        );

        document.reset_catalog_calls();
        reset_field_layer_preparation_counts();
        map_camera.reset(projection);
        assert!(map_camera.zoom_by(projection, 2.0));
        let medium = reconcile_spherical_document_camera(
            &document,
            &low,
            SphericalViewMode::Map,
            projection,
            map_camera,
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert!(!Arc::ptr_eq(&low, &medium));
        assert_eq!(medium.glyph_lod_key(), GlyphLodKey::Medium);
        assert_only_vector_glyph_revision_changed(&low, &medium);
        assert_eq!(document.catalog_calls(), 1);
        assert_eq!(
            field_layer_preparation_counts().diagnostic_validation_values_scanned,
            expected_diagnostic_scans
        );
        assert_eq!(
            field_layer_preparation_counts().diagnostic_fingerprint_values_scanned,
            expected_diagnostic_scans
        );

        document.reset_catalog_calls();
        reset_field_layer_preparation_counts();
        let clock_before_in_band = next_revision(&clock);
        let mut globe_camera = GlobeCamera::default();
        assert!(globe_camera.set_orthographic_scale(2.5));
        let globe_in_band = reconcile_spherical_document_camera(
            &document,
            &medium,
            SphericalViewMode::Globe,
            projection,
            map_camera,
            globe_camera,
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_eq!(state.vector_view_zoom(), 2.5);
        assert!(Arc::ptr_eq(&medium, &globe_in_band));
        assert_eq!(document.catalog_calls(), 0);
        assert_eq!(
            field_layer_preparation_counts(),
            crate::view::FieldLayerPreparationCounts::default()
        );
        assert_eq!(next_revision(&clock), clock_before_in_band);

        let repeated = reconcile_spherical_document_camera(
            &document,
            &globe_in_band,
            SphericalViewMode::Globe,
            projection,
            map_camera,
            globe_camera,
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert!(Arc::ptr_eq(&globe_in_band, &repeated));
        assert_eq!(document.catalog_calls(), 0);
        assert_eq!(
            field_layer_preparation_counts(),
            crate::view::FieldLayerPreparationCounts::default()
        );

        document.reset_catalog_calls();
        reset_field_layer_preparation_counts();
        assert!(globe_camera.set_orthographic_scale(4.0));
        let high = reconcile_spherical_document_camera(
            &document,
            &repeated,
            SphericalViewMode::Globe,
            projection,
            map_camera,
            globe_camera,
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert!(!Arc::ptr_eq(&repeated, &high));
        assert_eq!(high.glyph_lod_key(), GlyphLodKey::High);
        assert_only_vector_glyph_revision_changed(&repeated, &high);
        assert_eq!(document.catalog_calls(), 1);
        assert_eq!(
            field_layer_preparation_counts().diagnostic_validation_values_scanned,
            expected_diagnostic_scans
        );
        assert_eq!(
            field_layer_preparation_counts().diagnostic_fingerprint_values_scanned,
            expected_diagnostic_scans
        );

        document.reset_catalog_calls();
        reset_field_layer_preparation_counts();
        assert!(globe_camera.set_orthographic_scale(4.5));
        let high_in_band = reconcile_spherical_document_camera(
            &document,
            &high,
            SphericalViewMode::Globe,
            projection,
            map_camera,
            globe_camera,
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_eq!(state.vector_view_zoom(), 4.5);
        assert!(Arc::ptr_eq(&high, &high_in_band));
        assert_eq!(document.catalog_calls(), 0);
        assert_eq!(
            field_layer_preparation_counts(),
            crate::view::FieldLayerPreparationCounts::default()
        );
    }

    #[test]
    fn camera_only_reconciliation_never_reuses_layers_from_another_source() {
        let first_outcome = build_outcome_with_seed(ROOT_SEED, 6_371_000.0);
        let second_outcome = build_outcome_with_seed(RootSeed::new(77), 6_371_000.0);
        let first = SphericalNaturalFieldDocument::from_build_outcome(&first_outcome).unwrap();
        let second = SphericalNaturalFieldDocument::from_build_outcome(&second_outcome).unwrap();
        assert_eq!(
            first.surface.snapshot().cells().len(),
            second.surface.snapshot().cells().len()
        );
        assert_ne!(first.presentation_source(), second.presentation_source());
        let first = CountingSphericalLayerDocument::new(&first);
        let second = CountingSphericalLayerDocument::new(&second);
        let mut state = SphericalFieldDisplayState::default();
        state.select_overlay(Some(plate_velocity_field_id()));
        state.set_vector_lod(VectorGlyphLod::Low);
        let mut clock = DisplayRevisionClock::default();
        let projection = SphericalProjectionKind::EqualEarth;
        let mut camera = MapCamera::default();
        assert!(camera.zoom_by(projection, 2.5));
        let first_layers = Arc::new(
            prepare_spherical_document_layers(
                &first,
                SphericalViewMode::Map,
                projection,
                camera,
                GlobeCamera::default(),
                &mut state,
                &mut clock,
            )
            .unwrap(),
        );

        second.reset_catalog_calls();
        let replaced = reconcile_spherical_document_camera(
            &second,
            &first_layers,
            SphericalViewMode::Map,
            projection,
            camera,
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();

        assert!(!Arc::ptr_eq(&first_layers, &replaced));
        assert_eq!(replaced.source(), &second.presentation_source());
        assert_eq!(second.catalog_calls(), 1);
        assert_eq!(replaced.glyph_lod_key(), GlyphLodKey::Medium);
    }

    #[test]
    fn camera_only_reconciliation_falls_back_for_pending_layer_state() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let document = CountingSphericalLayerDocument::new(&document);
        let mut state = SphericalFieldDisplayState::default();
        state.select_overlay(Some(plate_velocity_field_id()));
        state.set_vector_lod(VectorGlyphLod::Low);
        let mut clock = DisplayRevisionClock::default();
        let projection = SphericalProjectionKind::EqualEarth;
        let mut camera = MapCamera::default();
        assert!(camera.zoom_by(projection, 2.5));
        let current = Arc::new(
            prepare_spherical_document_layers(
                &document,
                SphericalViewMode::Map,
                projection,
                camera,
                GlobeCamera::default(),
                &mut state,
                &mut clock,
            )
            .unwrap(),
        );

        state.select_fill(preliminary_mean_air_temperature_c_field_id());
        document.reset_catalog_calls();
        let updated = reconcile_spherical_document_camera(
            &document,
            &current,
            SphericalViewMode::Map,
            projection,
            camera,
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();

        assert!(!Arc::ptr_eq(&current, &updated));
        assert_eq!(document.catalog_calls(), 1);
        assert_eq!(
            updated.fill().field_id(),
            &preliminary_mean_air_temperature_c_field_id()
        );
        assert_ne!(current.revisions().fill, updated.revisions().fill);
    }

    #[test]
    fn retained_camera_layers_keep_renderer_validation_fixed_and_only_write_the_uniform() {
        let Some((device, queue)) = request_spherical_test_device() else {
            return;
        };
        let outcome = build_outcome(6_371_000.0);
        let mut document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        document.diagnostics.push(OwnedViewDiagnostic {
            severity: ViewDiagnosticSeverity::Warning,
            code: "test.camera-renderer-diagnostic".into(),
            field_id: Some(plate_velocity_field_id()),
            cell_id: Some(CellId::from_raw(0)),
            message: "renderer fast-path keeps this diagnostic unscanned".into(),
        });
        let expected_diagnostic_scans = document.diagnostics.len();
        let document = CountingSphericalLayerDocument::new(&document);
        let mut state = SphericalFieldDisplayState::default();
        state.select_overlay(Some(plate_velocity_field_id()));
        state.set_vector_lod(VectorGlyphLod::Low);
        let mut clock = DisplayRevisionClock::default();
        let projection_kind = SphericalProjectionKind::EqualEarth;
        let projection = SphericalProjection::new(projection_kind, 0.0).unwrap();
        let mut map_camera = MapCamera::default();
        assert!(map_camera.zoom_by(projection_kind, 1.99));
        let low = Arc::new(
            prepare_spherical_document_layers(
                &document,
                SphericalViewMode::Map,
                projection_kind,
                map_camera,
                GlobeCamera::default(),
                &mut state,
                &mut clock,
            )
            .unwrap(),
        );
        let source = low.source().clone();
        let map = Arc::new(
            PreparedProjectedMap::build(
                source.clone(),
                document.inner.surface.snapshot(),
                projection,
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap(),
        );
        let globe = Arc::new(
            PreparedGlobeMesh::build(
                source,
                document.inner.surface.snapshot(),
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap(),
        );
        let map_revision = DisplayRevision::new(700).unwrap();
        let globe_revision = DisplayRevision::new(701).unwrap();
        let packet = SphericalGpuPacket::new(
            Arc::clone(&map),
            map_revision,
            Arc::clone(&globe),
            globe_revision,
            Arc::clone(&low),
        );
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        validation_probe::reset();
        renderer.prepare_packet(&device, &queue, &packet).unwrap();
        let installed_scans = validation_probe::snapshot();
        let installed_uploads = renderer.upload_counters();
        let installed_overlay_arcs = installed_overlay_arc_ids(&renderer).unwrap();

        document.reset_catalog_calls();
        reset_field_layer_preparation_counts();
        map_camera.reset(projection_kind);
        assert!(map_camera.zoom_by(projection_kind, 2.0));
        let medium = reconcile_spherical_document_camera(
            &document,
            &low,
            SphericalViewMode::Map,
            projection_kind,
            map_camera,
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();
        let medium_packet = SphericalGpuPacket::new(
            Arc::clone(&map),
            map_revision,
            Arc::clone(&globe),
            globe_revision,
            Arc::clone(&medium),
        );
        renderer
            .prepare_packet(&device, &queue, &medium_packet)
            .unwrap();

        assert!(!Arc::ptr_eq(&low, &medium));
        assert_only_vector_glyph_revision_changed(&low, &medium);
        assert_eq!(document.catalog_calls(), 1);
        assert_eq!(
            field_layer_preparation_counts().diagnostic_validation_values_scanned,
            expected_diagnostic_scans
        );
        assert_eq!(
            field_layer_preparation_counts().diagnostic_fingerprint_values_scanned,
            expected_diagnostic_scans
        );
        let medium_overlay_arcs = installed_overlay_arc_ids(&renderer).unwrap();
        assert_ne!(medium_overlay_arcs, installed_overlay_arcs);
        let medium_uploads = renderer.upload_counters();
        assert_eq!(
            medium_uploads.map_overlay_instances,
            installed_uploads.map_overlay_instances + 1
        );
        assert_eq!(
            medium_uploads.globe_overlay_instances,
            installed_uploads.globe_overlay_instances + 1
        );
        let medium_scans = validation_probe::snapshot();
        assert_eq!(
            medium_scans.full_validations,
            installed_scans.full_validations + 1
        );

        document.reset_catalog_calls();
        reset_field_layer_preparation_counts();
        map_camera.reset(projection_kind);
        assert!(map_camera.zoom_by(projection_kind, 2.5));
        let retained = reconcile_spherical_document_camera(
            &document,
            &medium,
            SphericalViewMode::Map,
            projection_kind,
            map_camera,
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();
        let retained_packet = SphericalGpuPacket::new(
            Arc::clone(&map),
            map_revision,
            Arc::clone(&globe),
            globe_revision,
            Arc::clone(&retained),
        );
        renderer
            .prepare_packet(&device, &queue, &retained_packet)
            .unwrap();
        let viewport = [256, 128];
        renderer
            .prepare_map_frame_for_test(&queue, &retained_packet, map_camera, viewport)
            .unwrap();

        assert!(Arc::ptr_eq(&medium, &retained));
        assert_eq!(document.catalog_calls(), 0);
        assert_eq!(
            field_layer_preparation_counts(),
            crate::view::FieldLayerPreparationCounts::default()
        );
        assert_eq!(validation_probe::snapshot(), medium_scans);
        assert_eq!(
            installed_overlay_arc_ids(&renderer).unwrap(),
            medium_overlay_arcs
        );
        let after_in_band = renderer.upload_counters();
        assert_eq!(after_in_band.map_geometry, medium_uploads.map_geometry);
        assert_eq!(after_in_band.globe_geometry, medium_uploads.globe_geometry);
        assert_eq!(after_in_band.fill_field, medium_uploads.fill_field);
        assert_eq!(after_in_band.diagnostics, medium_uploads.diagnostics);
        assert_eq!(after_in_band.palettes, medium_uploads.palettes);
        assert_eq!(
            after_in_band.map_overlay_instances,
            medium_uploads.map_overlay_instances
        );
        assert_eq!(
            after_in_band.globe_overlay_instances,
            medium_uploads.globe_overlay_instances
        );
        assert_eq!(after_in_band.uniforms, medium_uploads.uniforms + 1);
        assert_eq!(
            after_in_band.uploaded_bytes,
            medium_uploads.uploaded_bytes + SphericalFieldRenderer::frame_uniform_size_for_test()
        );

        document.reset_catalog_calls();
        reset_field_layer_preparation_counts();
        map_camera.reset(projection_kind);
        assert!(map_camera.zoom_by(projection_kind, 4.0));
        let high = reconcile_spherical_document_camera(
            &document,
            &retained,
            SphericalViewMode::Map,
            projection_kind,
            map_camera,
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();
        let high_packet = SphericalGpuPacket::new(
            Arc::clone(&map),
            map_revision,
            Arc::clone(&globe),
            globe_revision,
            Arc::clone(&high),
        );
        renderer
            .prepare_packet(&device, &queue, &high_packet)
            .unwrap();

        assert!(!Arc::ptr_eq(&retained, &high));
        assert_only_vector_glyph_revision_changed(&retained, &high);
        assert_ne!(
            installed_overlay_arc_ids(&renderer).unwrap(),
            medium_overlay_arcs
        );
        let after_crossing = renderer.upload_counters();
        assert_eq!(
            after_crossing.map_overlay_instances,
            after_in_band.map_overlay_instances + 1
        );
        assert_eq!(
            after_crossing.globe_overlay_instances,
            after_in_band.globe_overlay_instances + 1
        );
        let crossing_scans = validation_probe::snapshot();
        assert_eq!(
            crossing_scans.full_validations,
            medium_scans.full_validations + 1
        );
        assert!(crossing_scans.cell_ids > medium_scans.cell_ids);
        assert!(crossing_scans.indices > medium_scans.indices);
        assert!(crossing_scans.positions > medium_scans.positions);

        document.reset_catalog_calls();
        reset_field_layer_preparation_counts();
        map_camera.reset(projection_kind);
        assert!(map_camera.zoom_by(projection_kind, 4.5));
        let high_retained = reconcile_spherical_document_camera(
            &document,
            &high,
            SphericalViewMode::Map,
            projection_kind,
            map_camera,
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();
        let high_retained_packet = SphericalGpuPacket::new(
            map,
            map_revision,
            globe,
            globe_revision,
            Arc::clone(&high_retained),
        );
        renderer
            .prepare_packet(&device, &queue, &high_retained_packet)
            .unwrap();
        let crossing_overlay_arcs = installed_overlay_arc_ids(&renderer).unwrap();
        renderer
            .prepare_map_frame_for_test(&queue, &high_retained_packet, map_camera, viewport)
            .unwrap();

        assert!(Arc::ptr_eq(&high, &high_retained));
        assert_eq!(state.vector_view_zoom(), 4.5);
        assert_eq!(document.catalog_calls(), 0);
        assert_eq!(
            field_layer_preparation_counts(),
            crate::view::FieldLayerPreparationCounts::default()
        );
        assert_eq!(validation_probe::snapshot(), crossing_scans);
        assert_eq!(
            installed_overlay_arc_ids(&renderer).unwrap(),
            crossing_overlay_arcs
        );
        let after_high_in_band = renderer.upload_counters();
        assert_eq!(
            after_high_in_band.map_overlay_instances,
            after_crossing.map_overlay_instances
        );
        assert_eq!(
            after_high_in_band.globe_overlay_instances,
            after_crossing.globe_overlay_instances
        );
        assert_eq!(after_high_in_band.uniforms, after_crossing.uniforms + 1);
    }

    #[test]
    fn edge_preparation_reports_field_ids_for_bad_cardinality_and_channels() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let catalog = payload_catalog(&document);
        let edge_count = document.surface.snapshot().edges().len();
        let boundary = catalog
            .get(&boundary_kind_field_id())
            .unwrap()
            .view()
            .unwrap();
        assert!(matches!(
            prepare_edge_field(boundary, edge_count - 1, crate::view::DisplayRangeMode::Data),
            Err(DisplayPrepareError::FieldCardinalityMismatch { field, .. })
                if field == boundary_kind_field_id()
        ));
        let elevation = catalog
            .get(&surface_elevation_m_field_id())
            .unwrap()
            .view()
            .unwrap();
        assert!(matches!(
            prepare_edge_field(elevation, edge_count, crate::view::DisplayRangeMode::Data),
            Err(DisplayPrepareError::UnsupportedSphericalChannel { field })
                if field == surface_elevation_m_field_id()
        ));
    }

    #[test]
    fn failed_spherical_preparation_preserves_state_and_revision_clock() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let catalog = payload_catalog(&document);
        let cell_count = document.surface.snapshot().cells().len();
        let mut state = SphericalFieldDisplayState::default();
        state.select_overlay(Some(surface_elevation_m_field_id()));
        state.select_entity(Some(crate::view::SelectedSurfaceEntity::Cell(
            CellId::from_raw(cell_count as u32),
        )));
        let before_state = state.clone();
        let mut clock = crate::view::DisplayRevisionClock::default();
        let mut expected_clock = clock.clone();
        let invalid_diagnostics = [OwnedViewDiagnostic {
            severity: ViewDiagnosticSeverity::Error,
            code: "test.invalid-cell".into(),
            field_id: None,
            cell_id: Some(CellId::from_raw(cell_count as u32)),
            message: "outside the spherical cell range".into(),
        }];

        assert!(prepare_spherical_field_layers(
            document.presentation_source(),
            &catalog,
            cell_count,
            document.surface.snapshot().edges().len(),
            &invalid_diagnostics,
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .is_err());

        assert_eq!(state, before_state);
        assert_eq!(
            clock.issue().unwrap().get(),
            expected_clock.issue().unwrap().get()
        );
    }

    #[test]
    fn overlay_palette_uses_its_own_schema_not_the_fill_override() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let catalog = payload_catalog(&document);
        let cell_count = document.surface.snapshot().cells().len();
        let edge_count = document.surface.snapshot().edges().len();
        let mut state = SphericalFieldDisplayState::default();
        state.select_fill(surface_elevation_m_field_id());
        state.set_palette_override(Some(PaletteId::Diverging));
        state.select_overlay(Some(boundary_kind_field_id()));
        let mut clock = crate::view::DisplayRevisionClock::default();
        let category = prepare_spherical_field_layers(
            document.presentation_source(),
            &catalog,
            cell_count,
            edge_count,
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_eq!(
            category.fill_palette(),
            built_in_palette(PaletteId::Diverging)
        );
        assert_eq!(
            category.overlay_palette().unwrap(),
            built_in_palette(PaletteId::Categorical)
        );

        state.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
        let vector = prepare_spherical_field_layers(
            document.presentation_source(),
            &catalog,
            cell_count,
            edge_count,
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_eq!(
            vector.overlay_palette().unwrap(),
            built_in_palette(PaletteId::Sequential)
        );
    }

    #[test]
    fn failed_spherical_update_preserves_state_and_revision_clock() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let catalog = payload_catalog(&document);
        let cell_count = document.surface.snapshot().cells().len();
        let edge_count = document.surface.snapshot().edges().len();
        let mut state = SphericalFieldDisplayState::default();
        let mut clock = crate::view::DisplayRevisionClock::default();
        let current = prepare_spherical_field_layers(
            document.presentation_source(),
            &catalog,
            cell_count,
            edge_count,
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();
        state.select_overlay(Some(surface_elevation_m_field_id()));
        state.select_entity(Some(crate::view::SelectedSurfaceEntity::Cell(
            CellId::from_raw(cell_count as u32),
        )));
        let before_state = state.clone();
        let mut expected_clock = clock.clone();
        let invalid_diagnostics = [OwnedViewDiagnostic {
            severity: ViewDiagnosticSeverity::Error,
            code: "test.invalid-cell".into(),
            field_id: None,
            cell_id: Some(CellId::from_raw(cell_count as u32)),
            message: "outside the spherical cell range".into(),
        }];

        assert!(crate::view::update_spherical_field_layers(
            &current,
            document.presentation_source(),
            &catalog,
            cell_count,
            edge_count,
            &invalid_diagnostics,
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .is_err());

        assert_eq!(state, before_state);
        assert_eq!(
            clock.issue().unwrap().get(),
            expected_clock.issue().unwrap().get()
        );
    }

    #[test]
    fn spherical_updates_prepare_only_changed_large_payloads() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let catalog = payload_catalog(&document);
        let cell_count = document.surface.snapshot().cells().len();
        let edge_count = document.surface.snapshot().edges().len();
        let expected_diagnostic_scans = document.diagnostics().len();
        let mut state = SphericalFieldDisplayState::default();
        state.select_fill(surface_elevation_m_field_id());
        state.select_overlay(Some(boundary_kind_field_id()));
        let mut clock = crate::view::DisplayRevisionClock::default();
        let initial = prepare_spherical_field_layers(
            document.presentation_source(),
            &catalog,
            cell_count,
            edge_count,
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();

        crate::view::reset_field_layer_preparation_counts();
        state.select_fill(elevation_field_id());
        let fill_changed = crate::view::update_spherical_field_layers(
            &initial,
            document.presentation_source(),
            &catalog,
            cell_count,
            edge_count,
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_eq!(
            crate::view::field_layer_preparation_counts(),
            crate::view::FieldLayerPreparationCounts {
                fill: 1,
                overlay: 0,
                diagnostics: 1,
                diagnostic_validation_values_scanned: expected_diagnostic_scans,
                diagnostic_fingerprint_values_scanned: expected_diagnostic_scans,
            }
        );

        crate::view::reset_field_layer_preparation_counts();
        state.select_overlay(Some(boundary_strength_field_id()));
        let _ = crate::view::update_spherical_field_layers(
            &fill_changed,
            document.presentation_source(),
            &catalog,
            cell_count,
            edge_count,
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_eq!(
            crate::view::field_layer_preparation_counts(),
            crate::view::FieldLayerPreparationCounts {
                fill: 0,
                overlay: 1,
                diagnostics: 0,
                diagnostic_validation_values_scanned: expected_diagnostic_scans,
                diagnostic_fingerprint_values_scanned: expected_diagnostic_scans,
            }
        );
    }

    #[test]
    fn switching_fill_reconciles_range_against_the_published_field() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let mut state = SphericalFieldDisplayState::default();
        let mut clock = DisplayRevisionClock::default();
        let initial = prepare_spherical_document_layers(
            &document,
            SphericalViewMode::Map,
            SphericalProjectionKind::EqualEarth,
            MapCamera::default(),
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();
        let elevation_range = initial.fill().display_range().unwrap().bounds();

        state.select_fill(crust_thickness_field_id());
        let switched = update_spherical_document_layers(
            &document,
            &initial,
            SphericalViewMode::Map,
            SphericalProjectionKind::EqualEarth,
            MapCamera::default(),
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();

        let thickness = document.tectonic.snapshot().crust_thickness_km();
        let expected = (
            thickness.iter().copied().fold(f32::INFINITY, f32::min),
            thickness.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        );
        assert_eq!(state.range_mode(), DisplayRangeMode::Data);
        assert_eq!(switched.fill().field_id(), &crust_thickness_field_id());
        assert_eq!(switched.fill().display_range().unwrap().bounds(), expected);
        assert_ne!(expected, elevation_range);
    }

    #[test]
    fn spherical_updates_refresh_only_changed_diagnostics() {
        let outcome = build_outcome(6_371_000.0);
        let mut document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        document.diagnostics.clear();
        let mut state = SphericalFieldDisplayState::default();
        state.select_fill(surface_elevation_m_field_id());
        state.select_overlay(Some(boundary_kind_field_id()));
        let mut clock = crate::view::DisplayRevisionClock::default();
        let initial = prepare_spherical_document_layers(
            &document,
            SphericalViewMode::Map,
            SphericalProjectionKind::EqualEarth,
            MapCamera::default(),
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();
        document.diagnostics.push(OwnedViewDiagnostic {
            severity: ViewDiagnosticSeverity::Warning,
            code: "test.changed-diagnostic".into(),
            field_id: None,
            cell_id: Some(CellId::from_raw(0)),
            message: "a valid changed diagnostic".into(),
        });

        let changed = update_spherical_document_layers(
            &document,
            &initial,
            SphericalViewMode::Map,
            SphericalProjectionKind::EqualEarth,
            MapCamera::default(),
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_eq!(changed.diagnostics().cells()[0], 2);
        assert!(!std::sync::Arc::ptr_eq(
            initial.diagnostics_arc(),
            changed.diagnostics_arc()
        ));
        assert_ne!(
            initial.revisions().diagnostics,
            changed.revisions().diagnostics
        );
        assert!(std::sync::Arc::ptr_eq(
            initial.fill_arc(),
            changed.fill_arc()
        ));
        assert!(std::sync::Arc::ptr_eq(
            initial.fill_palette_arc(),
            changed.fill_palette_arc()
        ));
        let (PreparedSphericalOverlay::Edge(before), PreparedSphericalOverlay::Edge(after)) =
            (initial.overlay().unwrap(), changed.overlay().unwrap())
        else {
            panic!("the unchanged overlay must remain an edge field");
        };
        assert!(std::sync::Arc::ptr_eq(before, after));
        assert_eq!(initial.revisions().fill, changed.revisions().fill);
        assert_eq!(initial.revisions().overlay, changed.revisions().overlay);
        assert_eq!(
            initial.revisions().fill_palette,
            changed.revisions().fill_palette
        );
        assert_eq!(
            initial.revisions().overlay_palette,
            changed.revisions().overlay_palette
        );

        let identical = update_spherical_document_layers(
            &document,
            &changed,
            SphericalViewMode::Map,
            SphericalProjectionKind::EqualEarth,
            MapCamera::default(),
            GlobeCamera::default(),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert!(std::sync::Arc::ptr_eq(
            changed.diagnostics_arc(),
            identical.diagnostics_arc()
        ));
        assert_eq!(
            changed.revisions().diagnostics,
            identical.revisions().diagnostics
        );
    }

    #[test]
    fn document_identity_comes_only_from_verified_outcome_provenance() {
        let alternate_seed = RootSeed::new(77);
        let outcome = build_outcome_with_seed(alternate_seed, 6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();

        assert_eq!(document.identity().root_seed(), alternate_seed);

        let mut artifact_mismatch = build_outcome(6_371_000.0);
        artifact_mismatch.artifacts = build_outcome(7_000_000.0).artifacts;
        assert!(matches!(
            SphericalNaturalFieldDocument::from_build_outcome(&artifact_mismatch),
            Err(SphericalNaturalDisplayError::BuildOutcomeIntegrity(
                BuildOutcomeIntegrityError::ArtifactSetMismatch { .. }
            ))
        ));

        let mut report_mismatch = build_outcome(6_371_000.0);
        report_mismatch.report = build_outcome(7_000_000.0).report;
        assert!(matches!(
            SphericalNaturalFieldDocument::from_build_outcome(&report_mismatch),
            Err(SphericalNaturalDisplayError::BuildOutcomeIntegrity(
                BuildOutcomeIntegrityError::ReportResultHashMismatch { .. }
            ))
        ));
    }

    #[test]
    fn document_publishes_every_payload_with_surface_cardinality_and_borrowed_storage() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let catalog = payload_catalog(&document);
        let cell_count = document.surface.snapshot().cells().len();
        let edge_count = document.surface.snapshot().edges().len();

        assert_eq!(catalog.entries().len(), 36);
        assert_eq!(
            catalog
                .get(&plate_id_field_id())
                .unwrap()
                .schema()
                .category_labels
                .len(),
            document.tectonic.snapshot().plates().len(),
            "the display registry must describe the evolved final plate table"
        );
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
        let first = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let second = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let first_hash = field_hash(&first);
        let second_hash = field_hash(&second);

        println!("spherical_natural_field_hash={first_hash}");
        assert_eq!(first_hash, second_hash);
        assert_eq!(first_hash, EXPECTED_FIELD_HASH);
    }

    #[test]
    fn local_east_north_vectors_reconstruct_authoritative_tangent_vectors() {
        let outcome = build_outcome(6_371_000.0);
        let document = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
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
            *first.verified_provenance().unwrap(),
            artifacts.surface,
            artifacts.formation,
            artifacts.resolved_tectonic,
            artifacts.tectonic,
            artifacts.mantle,
            artifacts.relief,
            artifacts.relief_spec,
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
            SphericalNaturalFieldDocument::from_build_outcome(&outcome),
            Err(SphericalNaturalDisplayError::MissingBuildResultHash)
        ));
    }

    #[test]
    fn rebuilding_the_document_reuses_artifacts_and_recreates_only_disposable_vectors() {
        let outcome = build_outcome(6_371_000.0);
        let first = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();
        let second = SphericalNaturalFieldDocument::from_build_outcome(&outcome).unwrap();

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
            Arc::new(SphericalNaturalFieldDocument::from_build_outcome(&valid).unwrap());
        let before = Arc::clone(&published);
        let mut invalid = build_outcome(7_000_000.0);
        invalid.report = BuildReport::new();

        assert!(matches!(
            try_replace_spherical_natural_document(&mut published, &invalid),
            Err(SphericalNaturalDisplayError::MissingBuildResultHash)
        ));
        assert!(Arc::ptr_eq(&published, &before));

        let replacement = build_outcome(7_000_000.0);
        try_replace_spherical_natural_document(&mut published, &replacement).unwrap();
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
