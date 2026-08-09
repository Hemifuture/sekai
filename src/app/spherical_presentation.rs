//! Composition root for one complete source-bound spherical presentation.

use std::sync::Arc;

use eframe::egui_wgpu::wgpu;
use thiserror::Error;

use super::field_document::{
    prepare_spherical_document_layers, reconcile_spherical_document_camera,
    update_spherical_document_layers,
};
use super::spherical_natural_display::{
    SphericalNaturalDisplayError, SphericalNaturalFieldDocument,
};
use crate::engine::{
    ArtifactError, BuildEngine, BuildFailure, BuildReport, ExternalArtifacts, GraphError,
    MemoryStageCache,
};
use crate::generators::natural::{
    spherical_natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact,
    GeologicSpecArtifact, HydroErosionSpecArtifact, RulePackSetArtifact, TectonicSpecArtifact,
    WorldFormationSpecArtifact,
};
use crate::generators::spatial::SphericalSpaceArtifact;
use crate::gpu::spherical::{SphericalFieldRenderer, SphericalGpuPacket, SphericalRenderError};
use crate::rules::{default_rule_pack_set, AuthorConstraints, BuiltinRuleError};
use crate::view::{
    DisplayPrepareError, DisplayRevision, DisplayRevisionClock, FieldLayerRevisions, GlobeCamera,
    MapCamera, PreparedFieldLayers, PreparedGlobeMesh, PreparedProjectedMap,
    SphericalEntityLocator, SphericalFieldDisplayState, SphericalMeshBudgets, SphericalMeshError,
    SphericalPickingError, SphericalPresentationSource, SphericalPresentationViewState,
    SphericalProjection, SphericalProjectionError, SphericalViewMode,
};
use crate::world::natural::{
    GeologicSpec, GeologicSpecError, NaturalSpecError, TectonicSpec, WorldFormationSpec,
    WorldFormationSpecError,
};
use crate::world::{RootSeed, SphericalSpaceSpec, SphericalSpecError};

/// The renderer-side preparation gate that must succeed before CPU publication changes.
#[derive(Clone, Copy)]
enum SphericalGpuPreparation<'a> {
    Initial,
    Replacement { expected: &'a SphericalGpuPacket },
}

trait SphericalGpuPreparer {
    /// Rejects the wrong renderer owner before candidate or GPU validation performs work.
    fn preflight(
        &self,
        preparation: SphericalGpuPreparation<'_>,
    ) -> Result<(), SphericalRenderError>;

    /// Atomically prepares the complete GPU packet or retains the previous renderer state.
    fn prepare(
        &mut self,
        preparation: SphericalGpuPreparation<'_>,
        packet: &SphericalGpuPacket,
    ) -> Result<(), SphericalRenderError>;
}

/// Production adapter from publication orchestration to the spherical wgpu renderer.
pub struct SphericalRendererPreparer<'a> {
    renderer: &'a mut SphericalFieldRenderer,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    #[cfg(test)]
    fail_next_prepare: bool,
}

impl<'a> SphericalRendererPreparer<'a> {
    /// Binds the renderer and device resources used by one publication lineage.
    ///
    /// Initial publication requires an empty renderer. Every later replacement requires that
    /// renderer to still contain the exact current packet owned by the publication being changed.
    pub const fn new(
        renderer: &'a mut SphericalFieldRenderer,
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
    ) -> Self {
        Self {
            renderer,
            device,
            queue,
            #[cfg(test)]
            fail_next_prepare: false,
        }
    }

    #[cfg(test)]
    pub(super) fn fail_next_prepare_for_test(&mut self) {
        self.fail_next_prepare = true;
    }
}

impl SphericalGpuPreparer for SphericalRendererPreparer<'_> {
    fn preflight(
        &self,
        preparation: SphericalGpuPreparation<'_>,
    ) -> Result<(), SphericalRenderError> {
        match preparation {
            SphericalGpuPreparation::Initial => self.renderer.ensure_publication_uninitialized(),
            SphericalGpuPreparation::Replacement { expected } => {
                self.renderer.ensure_publication_current(expected)
            }
        }
    }

    fn prepare(
        &mut self,
        preparation: SphericalGpuPreparation<'_>,
        packet: &SphericalGpuPacket,
    ) -> Result<(), SphericalRenderError> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_prepare) {
            return Err(SphericalRenderError::BufferLimitExceeded {
                resource: "injected Task 10 migration GPU preparation",
                required: 1,
                max: 0,
            });
        }
        match preparation {
            SphericalGpuPreparation::Initial => {
                self.renderer
                    .prepare_initial_publication_packet(self.device, self.queue, packet)
            }
            SphericalGpuPreparation::Replacement { expected } => self
                .renderer
                .prepare_replacement_publication_packet(self.device, self.queue, expected, packet),
        }
    }
}

/// A validated projected-map presenter handle sharing one immutable field allocation.
#[derive(Clone)]
pub struct SphericalMapPresenter {
    map: Arc<PreparedProjectedMap>,
    layers: Arc<PreparedFieldLayers>,
}

impl SphericalMapPresenter {
    /// Rejects source mixing even when geometry and field cardinalities happen to match.
    pub fn try_new(
        map: Arc<PreparedProjectedMap>,
        layers: Arc<PreparedFieldLayers>,
    ) -> Result<Self, SphericalPresentationError> {
        if map.source() != layers.source() {
            return Err(SphericalPresentationError::SourceMismatch {
                resource: "projected map",
            });
        }
        validate_cardinality("projected map", layers.fill().len(), map.cell_count())?;
        Ok(Self { map, layers })
    }

    /// Returns the source-bound projected geometry.
    pub fn map_arc(&self) -> &Arc<PreparedProjectedMap> {
        &self.map
    }

    /// Returns the exact shared field-layer allocation.
    pub fn layers_arc(&self) -> &Arc<PreparedFieldLayers> {
        &self.layers
    }
}

/// A validated unit-globe presenter handle sharing one immutable field allocation.
#[derive(Clone)]
pub struct SphericalGlobePresenter {
    globe: Arc<PreparedGlobeMesh>,
    layers: Arc<PreparedFieldLayers>,
}

impl SphericalGlobePresenter {
    /// Rejects source mixing even when geometry and field cardinalities happen to match.
    pub fn try_new(
        globe: Arc<PreparedGlobeMesh>,
        layers: Arc<PreparedFieldLayers>,
    ) -> Result<Self, SphericalPresentationError> {
        if globe.source() != layers.source() {
            return Err(SphericalPresentationError::SourceMismatch {
                resource: "unit globe",
            });
        }
        validate_cardinality("unit globe", layers.fill().len(), globe.cell_count())?;
        Ok(Self { globe, layers })
    }

    /// Returns the undeformed unit-globe geometry.
    pub fn globe_arc(&self) -> &Arc<PreparedGlobeMesh> {
        &self.globe
    }

    /// Returns the exact shared field-layer allocation.
    pub fn layers_arc(&self) -> &Arc<PreparedFieldLayers> {
        &self.layers
    }
}

/// One fully prepared world that is safe to publish as a single value.
///
/// A complete world candidate is a consume-once publication capability. It cannot be cloned into
/// two independent first publications with the same source, revisions, and clock.
///
/// ```compile_fail
/// fn fork_candidate(
///     candidate: sekai::app::SphericalPresentationCandidate,
///     first_gpu: &mut sekai::app::SphericalRendererPreparer<'_>,
///     second_gpu: &mut sekai::app::SphericalRendererPreparer<'_>,
/// ) {
///     let duplicate = candidate.clone();
///     let _ = sekai::app::PublishedSphericalPresentation::try_new(candidate, first_gpu);
///     let _ = sekai::app::PublishedSphericalPresentation::try_new(duplicate, second_gpu);
/// }
/// ```
///
/// ```compile_fail
/// fn publish_candidate_twice(
///     candidate: sekai::app::SphericalPresentationCandidate,
///     gpu: &mut sekai::app::SphericalRendererPreparer<'_>,
/// ) {
///     let _ = sekai::app::PublishedSphericalPresentation::try_new(candidate, gpu);
///     let _ = sekai::app::PublishedSphericalPresentation::try_new(candidate, gpu);
/// }
/// ```
pub struct SphericalPresentationCandidate {
    lineage: WorldCandidateLineage,
    document: Arc<SphericalNaturalFieldDocument>,
    source: SphericalPresentationSource,
    locator: Arc<SphericalEntityLocator>,
    map_presenter: SphericalMapPresenter,
    globe_presenter: SphericalGlobePresenter,
    layers: Arc<PreparedFieldLayers>,
    gpu_packet: Arc<SphericalGpuPacket>,
    view_state: SphericalPresentationViewState,
    state: SphericalFieldDisplayState,
    clock: DisplayRevisionClock,
    report: BuildReport,
}

impl SphericalPresentationCandidate {
    fn validate(&self) -> Result<(), SphericalPresentationError> {
        validate_complete_source_set(
            &self.document,
            &self.source,
            &self.locator,
            self.map(),
            self.globe(),
            &self.layers,
            &self.gpu_packet,
        )?;
        if !Arc::ptr_eq(self.map_presenter.layers_arc(), &self.layers)
            || !Arc::ptr_eq(self.globe_presenter.layers_arc(), &self.layers)
            || !Arc::ptr_eq(self.gpu_packet.layers_arc(), &self.layers)
        {
            return Err(SphericalPresentationError::FieldLayersNotShared);
        }
        if self.map().projection() != self.view_state.projection()
            || self.state.vector_view_zoom() != self.view_state.active_zoom()
        {
            return Err(SphericalPresentationError::ViewStateMismatch);
        }
        Ok(())
    }

    /// Returns the immutable source document.
    pub fn document(&self) -> &SphericalNaturalFieldDocument {
        &self.document
    }

    /// Returns the shared document allocation.
    pub fn document_arc(&self) -> &Arc<SphericalNaturalFieldDocument> {
        &self.document
    }

    /// Returns the verified presentation source derived from the document.
    pub const fn source(&self) -> &SphericalPresentationSource {
        &self.source
    }

    /// Returns the shared entity locator.
    pub fn locator(&self) -> &SphericalEntityLocator {
        &self.locator
    }

    /// Returns the shared entity-locator allocation.
    pub fn locator_arc(&self) -> &Arc<SphericalEntityLocator> {
        &self.locator
    }

    /// Returns the projected map.
    pub fn map(&self) -> &PreparedProjectedMap {
        self.map_presenter.map_arc()
    }

    /// Returns the projected-map allocation.
    pub fn map_arc(&self) -> &Arc<PreparedProjectedMap> {
        self.map_presenter.map_arc()
    }

    /// Returns the unit globe.
    pub fn globe(&self) -> &PreparedGlobeMesh {
        self.globe_presenter.globe_arc()
    }

    /// Returns the unit-globe allocation.
    pub fn globe_arc(&self) -> &Arc<PreparedGlobeMesh> {
        self.globe_presenter.globe_arc()
    }

    /// Returns the shared map presenter handle.
    pub const fn map_presenter(&self) -> &SphericalMapPresenter {
        &self.map_presenter
    }

    /// Returns the shared globe presenter handle.
    pub const fn globe_presenter(&self) -> &SphericalGlobePresenter {
        &self.globe_presenter
    }

    /// Returns the shared prepared field layers.
    pub fn layers(&self) -> &PreparedFieldLayers {
        &self.layers
    }

    /// Returns the exact field-layer allocation shared by both presenters.
    pub const fn layers_arc(&self) -> &Arc<PreparedFieldLayers> {
        &self.layers
    }

    /// Returns the validated GPU-neutral packet.
    pub fn gpu_packet(&self) -> &SphericalGpuPacket {
        &self.gpu_packet
    }

    /// Returns the GPU-neutral packet allocation.
    pub const fn gpu_packet_arc(&self) -> &Arc<SphericalGpuPacket> {
        &self.gpu_packet
    }

    /// Returns the reconciled spherical display state.
    pub const fn state(&self) -> &SphericalFieldDisplayState {
        &self.state
    }

    /// Returns the complete renderer-neutral view bound to this candidate.
    pub const fn view_state(&self) -> &SphericalPresentationViewState {
        &self.view_state
    }

    /// Returns the candidate revision clock.
    pub const fn clock(&self) -> &DisplayRevisionClock {
        &self.clock
    }

    /// Returns the exact formal graph report.
    pub const fn report(&self) -> &BuildReport {
        &self.report
    }

    /// Returns every data-bearing revision in the complete packet.
    pub fn revisions(&self) -> (DisplayRevision, DisplayRevision, FieldLayerRevisions) {
        (
            self.gpu_packet.map_geometry_revision(),
            self.gpu_packet.globe_geometry_revision(),
            self.layers.revisions(),
        )
    }

    fn snapshot_for_update(&self) -> Self {
        Self {
            lineage: WorldCandidateLineage::NoBase,
            document: Arc::clone(&self.document),
            source: self.source.clone(),
            locator: Arc::clone(&self.locator),
            map_presenter: self.map_presenter.clone(),
            globe_presenter: self.globe_presenter.clone(),
            layers: Arc::clone(&self.layers),
            gpu_packet: Arc::clone(&self.gpu_packet),
            view_state: self.view_state,
            state: self.state.clone(),
            clock: self.clock.clone(),
            report: self.report.clone(),
        }
    }
}

/// The currently published complete spherical world and all of its presentation derivatives.
///
/// A publication is the unique owner of its revision clock and therefore cannot be cloned into
/// two independently advancing current worlds.
///
/// ```compile_fail
/// fn require_clone<T: Clone>() {}
/// require_clone::<sekai::app::PublishedSphericalPresentation>();
/// ```
pub struct PublishedSphericalPresentation {
    current: SphericalPresentationCandidate,
}

impl PublishedSphericalPresentation {
    /// Prepares the renderer and publishes an already complete first candidate.
    pub fn try_new(
        candidate: SphericalPresentationCandidate,
        gpu: &mut SphericalRendererPreparer<'_>,
    ) -> Result<Self, SphericalPresentationError> {
        Self::try_new_with_preparer(candidate, gpu)
    }

    fn try_new_with_preparer<P: SphericalGpuPreparer + ?Sized>(
        mut candidate: SphericalPresentationCandidate,
        gpu: &mut P,
    ) -> Result<Self, SphericalPresentationError> {
        candidate.lineage.validate_initial()?;
        let preparation = SphericalGpuPreparation::Initial;
        gpu.preflight(preparation)?;
        candidate.validate()?;
        gpu.prepare(preparation, candidate.gpu_packet())?;
        candidate.lineage.clear();
        Ok(Self { current: candidate })
    }

    /// Builds a whole-world candidate bound to this exact publication and revision lineage.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_replacement_candidate(
        &self,
        root_seed: RootSeed,
        space: &SphericalSpaceSpec,
        formation: &WorldFormationSpec,
        tectonic: &TectonicSpec,
        geologic: &GeologicSpec,
        cache: &mut MemoryStageCache,
        requested_state: &SphericalFieldDisplayState,
    ) -> Result<SphericalPresentationCandidate, SphericalPresentationError> {
        self.prepare_replacement_candidate_for_view(
            root_seed,
            space,
            formation,
            tectonic,
            geologic,
            cache,
            *self.view_state(),
            requested_state,
        )
    }

    /// Builds a whole-world candidate bound to an explicit complete view and this lineage.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_replacement_candidate_for_view(
        &self,
        root_seed: RootSeed,
        space: &SphericalSpaceSpec,
        formation: &WorldFormationSpec,
        tectonic: &TectonicSpec,
        geologic: &GeologicSpec,
        cache: &mut MemoryStageCache,
        requested_view: SphericalPresentationViewState,
        requested_state: &SphericalFieldDisplayState,
    ) -> Result<SphericalPresentationCandidate, SphericalPresentationError> {
        self.prepare_replacement_candidate_for_view_impl(
            root_seed,
            space,
            formation,
            tectonic,
            geologic,
            cache,
            requested_view,
            requested_state,
            FailureInjector::NONE,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg(test)]
    fn prepare_replacement_candidate_impl(
        &self,
        root_seed: RootSeed,
        space: &SphericalSpaceSpec,
        formation: &WorldFormationSpec,
        tectonic: &TectonicSpec,
        geologic: &GeologicSpec,
        cache: &mut MemoryStageCache,
        requested_state: &SphericalFieldDisplayState,
        failure: FailureInjector,
    ) -> Result<SphericalPresentationCandidate, SphericalPresentationError> {
        self.prepare_replacement_candidate_for_view_impl(
            root_seed,
            space,
            formation,
            tectonic,
            geologic,
            cache,
            *self.view_state(),
            requested_state,
            failure,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_replacement_candidate_for_view_impl(
        &self,
        root_seed: RootSeed,
        space: &SphericalSpaceSpec,
        formation: &WorldFormationSpec,
        tectonic: &TectonicSpec,
        geologic: &GeologicSpec,
        cache: &mut MemoryStageCache,
        requested_view: SphericalPresentationViewState,
        requested_state: &SphericalFieldDisplayState,
        failure: FailureInjector,
    ) -> Result<SphericalPresentationCandidate, SphericalPresentationError> {
        build_spherical_presentation_candidate_impl_with_lineage(
            root_seed,
            space,
            formation,
            tectonic,
            geologic,
            cache,
            requested_view,
            requested_state,
            self.clock(),
            failure,
            WorldCandidateLineage::Replacement(Arc::new(StateBoundCandidateBase::from_published(
                self,
            ))),
        )
    }

    /// Revalidates, prepares GPU resources, then replaces the whole publication once.
    ///
    /// The candidate must come from [`Self::prepare_replacement_candidate`] on this exact current
    /// publication. Standalone candidates are initial-publication inputs only.
    pub fn try_replace(
        &mut self,
        candidate: SphericalPresentationCandidate,
        gpu: &mut SphericalRendererPreparer<'_>,
    ) -> Result<(), SphericalPresentationError> {
        self.try_replace_with_preparer(candidate, gpu)
    }

    fn try_replace_with_preparer<P: SphericalGpuPreparer + ?Sized>(
        &mut self,
        mut candidate: SphericalPresentationCandidate,
        gpu: &mut P,
    ) -> Result<(), SphericalPresentationError> {
        let expected = Arc::clone(self.gpu_packet_arc());
        let preparation = SphericalGpuPreparation::Replacement {
            expected: &expected,
        };
        gpu.preflight(preparation)?;
        candidate.lineage.validate_current(self)?;
        candidate.validate()?;
        gpu.prepare(preparation, candidate.gpu_packet())?;
        candidate.lineage.clear();
        self.current = candidate;
        Ok(())
    }

    /// Builds a disposable projected-map candidate without changing the current map or clock.
    pub fn prepare_projection_candidate(
        &self,
        projection: SphericalProjection,
        budgets: SphericalMeshBudgets,
    ) -> Result<SphericalProjectionCandidate, SphericalPresentationError> {
        let current = *self.view_state();
        self.prepare_projection_candidate_for_view(
            SphericalPresentationViewState::new(
                current.mode(),
                projection,
                current.map_camera(),
                current.globe_camera(),
            ),
            budgets,
        )
    }

    /// Builds a projected-map candidate bound to the complete requested view.
    pub fn prepare_projection_candidate_for_view(
        &self,
        view_state: SphericalPresentationViewState,
        budgets: SphericalMeshBudgets,
    ) -> Result<SphericalProjectionCandidate, SphericalPresentationError> {
        let map = Arc::new(PreparedProjectedMap::build(
            self.source().clone(),
            self.document().surface.snapshot(),
            view_state.projection(),
            budgets,
        )?);
        let mut clock = self.clock().clone();
        let revision = clock.issue()?;
        let mut state = self.state().clone();
        let layers = reconcile_spherical_document_camera(
            self.document(),
            self.layers_arc(),
            view_state.mode(),
            view_state.projection().kind(),
            view_state.map_camera(),
            view_state.globe_camera(),
            &mut state,
            &mut clock,
        )?;
        SphericalProjectionCandidate::try_new(self, map, layers, view_state, state, revision, clock)
    }

    /// Replaces only the projected-map sub-cache after candidate validation succeeds.
    pub fn try_replace_projection_candidate(
        &mut self,
        candidate: SphericalProjectionCandidate,
        gpu: &mut SphericalRendererPreparer<'_>,
    ) -> Result<(), SphericalPresentationError> {
        self.try_replace_projection_candidate_with_preparer(candidate, gpu)
    }

    fn try_replace_projection_candidate_with_preparer<P: SphericalGpuPreparer + ?Sized>(
        &mut self,
        candidate: SphericalProjectionCandidate,
        gpu: &mut P,
    ) -> Result<(), SphericalPresentationError> {
        let expected = Arc::clone(self.gpu_packet_arc());
        let preparation = SphericalGpuPreparation::Replacement {
            expected: &expected,
        };
        gpu.preflight(preparation)?;
        candidate.base.validate_current(self, "projection")?;
        candidate.validate(self)?;
        let mut state = candidate.state;
        state.set_vector_paused(self.state().vector_paused());
        state
            .set_vector_display_speed(self.state().vector_display_speed())
            .map_err(|_| SphericalPresentationError::FieldStateMismatch)?;
        if !matches!(
            state.selected_entity(),
            Some(crate::view::SelectedSurfaceEntity::Cell(_))
        ) && !matches!(
            self.state().selected_entity(),
            Some(crate::view::SelectedSurfaceEntity::Cell(_))
        ) {
            state.select_entity(self.state().selected_entity());
        }
        let mut next = self.current.snapshot_for_update();
        next.map_presenter = candidate.map_presenter;
        next.globe_presenter = candidate.globe_presenter;
        next.layers = candidate.layers;
        next.gpu_packet = candidate.gpu_packet;
        next.view_state = candidate.view_state;
        next.state = state;
        next.clock = candidate.clock;
        next.validate()?;
        gpu.prepare(preparation, next.gpu_packet())?;
        self.current = next;
        Ok(())
    }

    /// Builds a disposable field-layer candidate without changing current state or revisions.
    pub fn prepare_field_candidate(
        &self,
        requested_state: SphericalFieldDisplayState,
        mode: SphericalViewMode,
        map_camera: MapCamera,
        globe_camera: GlobeCamera,
    ) -> Result<SphericalFieldCandidate, SphericalPresentationError> {
        self.prepare_field_candidate_for_view(
            requested_state,
            SphericalPresentationViewState::new(
                mode,
                self.map().projection(),
                map_camera,
                globe_camera,
            ),
        )
    }

    /// Builds a field-layer candidate bound to the complete requested view.
    pub fn prepare_field_candidate_for_view(
        &self,
        requested_state: SphericalFieldDisplayState,
        view_state: SphericalPresentationViewState,
    ) -> Result<SphericalFieldCandidate, SphericalPresentationError> {
        let mut state = requested_state;
        let mut clock = self.clock().clone();
        let layers = Arc::new(update_spherical_document_layers(
            self.document(),
            self.layers(),
            view_state.mode(),
            view_state.projection().kind(),
            view_state.map_camera(),
            view_state.globe_camera(),
            &mut state,
            &mut clock,
        )?);
        SphericalFieldCandidate::try_new_for_view(self, layers, view_state, state, clock)
    }

    /// Replaces only field-bound presenter and packet caches after validation succeeds.
    pub fn try_replace_field_candidate(
        &mut self,
        candidate: SphericalFieldCandidate,
        gpu: &mut SphericalRendererPreparer<'_>,
    ) -> Result<(), SphericalPresentationError> {
        self.try_replace_field_candidate_with_preparer(candidate, gpu)
    }

    fn try_replace_field_candidate_with_preparer<P: SphericalGpuPreparer + ?Sized>(
        &mut self,
        candidate: SphericalFieldCandidate,
        gpu: &mut P,
    ) -> Result<(), SphericalPresentationError> {
        let expected = Arc::clone(self.gpu_packet_arc());
        let preparation = SphericalGpuPreparation::Replacement {
            expected: &expected,
        };
        gpu.preflight(preparation)?;
        candidate.base.validate_current(self, "field")?;
        candidate.validate(self)?;
        let mut next = self.current.snapshot_for_update();
        next.map_presenter = candidate.map_presenter;
        next.globe_presenter = candidate.globe_presenter;
        next.layers = candidate.layers;
        next.gpu_packet = candidate.gpu_packet;
        next.view_state = candidate.view_state;
        next.state = candidate.state;
        next.clock = candidate.clock;
        next.validate()?;
        gpu.prepare(preparation, next.gpu_packet())?;
        self.current = next;
        Ok(())
    }

    /// Returns the immutable source document.
    pub fn document(&self) -> &SphericalNaturalFieldDocument {
        self.current.document()
    }

    /// Returns the shared document allocation.
    pub fn document_arc(&self) -> &Arc<SphericalNaturalFieldDocument> {
        self.current.document_arc()
    }

    /// Returns the verified source identity.
    pub const fn source(&self) -> &SphericalPresentationSource {
        self.current.source()
    }

    /// Returns the shared entity locator.
    pub fn locator(&self) -> &SphericalEntityLocator {
        self.current.locator()
    }

    /// Returns the shared entity-locator allocation.
    pub fn locator_arc(&self) -> &Arc<SphericalEntityLocator> {
        self.current.locator_arc()
    }

    /// Returns the projected map.
    pub fn map(&self) -> &PreparedProjectedMap {
        self.current.map()
    }

    /// Returns the projected-map allocation.
    pub fn map_arc(&self) -> &Arc<PreparedProjectedMap> {
        self.current.map_arc()
    }

    /// Returns the undeformed unit globe.
    pub fn globe(&self) -> &PreparedGlobeMesh {
        self.current.globe()
    }

    /// Returns the unit-globe allocation.
    pub fn globe_arc(&self) -> &Arc<PreparedGlobeMesh> {
        self.current.globe_arc()
    }

    /// Returns the shared field layers.
    pub fn layers(&self) -> &PreparedFieldLayers {
        self.current.layers()
    }

    /// Returns the exact field-layer allocation shared by both presenters.
    pub const fn layers_arc(&self) -> &Arc<PreparedFieldLayers> {
        self.current.layers_arc()
    }

    /// Returns the GPU-neutral packet.
    pub fn gpu_packet(&self) -> &SphericalGpuPacket {
        self.current.gpu_packet()
    }

    /// Returns the GPU-neutral packet allocation.
    pub const fn gpu_packet_arc(&self) -> &Arc<SphericalGpuPacket> {
        self.current.gpu_packet_arc()
    }

    /// Returns the reconciled display state.
    pub const fn state(&self) -> &SphericalFieldDisplayState {
        self.current.state()
    }

    /// Returns the complete renderer-neutral view bound to the current publication.
    pub const fn view_state(&self) -> &SphericalPresentationViewState {
        self.current.view_state()
    }

    /// Reconciles state that is already represented by the current layers or fixed uniforms.
    ///
    /// This is the O(1) path for camera, view, animation controls, and edge/none selection. It
    /// refuses any state whose effective layer-bearing inputs differ from the current packet.
    pub(crate) fn reconcile_uniform_only_state(
        &mut self,
        state: SphericalFieldDisplayState,
        view_state: SphericalPresentationViewState,
    ) -> Result<(), SphericalPresentationError> {
        if !self.current.layers.matches_camera_only_state(&state)
            || self.current.map().projection() != view_state.projection()
            || state.vector_view_zoom() != view_state.active_zoom()
        {
            return Err(SphericalPresentationError::FieldStateMismatch);
        }
        self.current.state = state;
        self.current.view_state = view_state;
        Ok(())
    }

    /// Returns the current revision clock.
    pub const fn clock(&self) -> &DisplayRevisionClock {
        self.current.clock()
    }

    /// Returns the report belonging to the published world.
    pub const fn report(&self) -> &BuildReport {
        self.current.report()
    }

    /// Returns every data-bearing revision in the complete packet.
    pub fn revisions(&self) -> (DisplayRevision, DisplayRevision, FieldLayerRevisions) {
        self.current.revisions()
    }
}

#[derive(Clone)]
struct CandidateBase {
    packet: Arc<SphericalGpuPacket>,
    revisions: (DisplayRevision, DisplayRevision, FieldLayerRevisions),
    clock: DisplayRevisionClock,
    view_state: SphericalPresentationViewState,
}

impl CandidateBase {
    fn from_published(published: &PublishedSphericalPresentation) -> Self {
        Self {
            packet: Arc::clone(published.gpu_packet_arc()),
            revisions: published.revisions(),
            clock: published.clock().clone(),
            view_state: *published.view_state(),
        }
    }

    fn validate_current(
        &self,
        published: &PublishedSphericalPresentation,
        candidate: &'static str,
    ) -> Result<(), SphericalPresentationError> {
        let mut base_clock = self.clock.clone();
        let mut current_clock = published.clock().clone();
        if !Arc::ptr_eq(&self.packet, published.gpu_packet_arc())
            || self.revisions != published.revisions()
            || self.view_state != *published.view_state()
            || base_clock.issue().ok() != current_clock.issue().ok()
        {
            return Err(SphericalPresentationError::StaleCandidate { candidate });
        }
        Ok(())
    }
}

#[derive(Clone)]
struct StateBoundCandidateBase {
    publication: CandidateBase,
    state: SphericalFieldDisplayState,
}

#[derive(Clone)]
enum WorldCandidateLineage {
    NoBase,
    Replacement(Arc<StateBoundCandidateBase>),
}

impl WorldCandidateLineage {
    fn validate_initial(&self) -> Result<(), SphericalPresentationError> {
        match self {
            Self::NoBase => Ok(()),
            Self::Replacement(_) => {
                Err(SphericalPresentationError::InitialCandidateRequiresStandalone)
            }
        }
    }

    fn validate_current(
        &self,
        published: &PublishedSphericalPresentation,
    ) -> Result<(), SphericalPresentationError> {
        match self {
            Self::NoBase => Err(SphericalPresentationError::StaleCandidate { candidate: "world" }),
            Self::Replacement(base) => base.validate_current(published, "world"),
        }
    }

    fn clear(&mut self) {
        *self = Self::NoBase;
    }
}

impl StateBoundCandidateBase {
    fn from_published(published: &PublishedSphericalPresentation) -> Self {
        Self {
            publication: CandidateBase::from_published(published),
            state: published.state().clone(),
        }
    }

    fn validate_current(
        &self,
        published: &PublishedSphericalPresentation,
        candidate: &'static str,
    ) -> Result<(), SphericalPresentationError> {
        self.publication.validate_current(published, candidate)?;
        if &self.state != published.state() {
            return Err(SphericalPresentationError::StaleCandidate { candidate });
        }
        Ok(())
    }
}

/// A complete replacement for only the projected-map sub-cache.
pub struct SphericalProjectionCandidate {
    base: CandidateBase,
    source: SphericalPresentationSource,
    map_presenter: SphericalMapPresenter,
    globe_presenter: SphericalGlobePresenter,
    layers: Arc<PreparedFieldLayers>,
    gpu_packet: Arc<SphericalGpuPacket>,
    view_state: SphericalPresentationViewState,
    state: SphericalFieldDisplayState,
    clock: DisplayRevisionClock,
}

impl SphericalProjectionCandidate {
    fn try_new(
        published: &PublishedSphericalPresentation,
        map: Arc<PreparedProjectedMap>,
        layers: Arc<PreparedFieldLayers>,
        view_state: SphericalPresentationViewState,
        state: SphericalFieldDisplayState,
        revision: DisplayRevision,
        clock: DisplayRevisionClock,
    ) -> Result<Self, SphericalPresentationError> {
        let base = CandidateBase::from_published(published);
        let source = published.source().clone();
        if map.source() != &source {
            return Err(SphericalPresentationError::SourceMismatch {
                resource: "projected map",
            });
        }
        let map_presenter = SphericalMapPresenter::try_new(Arc::clone(&map), Arc::clone(&layers))?;
        let globe_presenter = SphericalGlobePresenter::try_new(
            Arc::clone(published.globe_arc()),
            Arc::clone(&layers),
        )?;
        let gpu_packet = Arc::new(SphericalGpuPacket::try_new(
            map,
            revision,
            Arc::clone(published.globe_arc()),
            published.gpu_packet().globe_geometry_revision(),
            Arc::clone(&layers),
        )?);
        Ok(Self {
            base,
            source,
            map_presenter,
            globe_presenter,
            layers,
            gpu_packet,
            view_state,
            state,
            clock,
        })
    }

    fn validate(
        &self,
        published: &PublishedSphericalPresentation,
    ) -> Result<(), SphericalPresentationError> {
        if &self.source != published.source() {
            return Err(SphericalPresentationError::SourceMismatch {
                resource: "projection candidate",
            });
        }
        if self.map_presenter.map_arc().projection() != self.view_state.projection() {
            return Err(SphericalPresentationError::ViewStateMismatch);
        }
        if self.map_presenter.map_arc().source() != &self.source
            || self.layers.source() != &self.source
            || self.gpu_packet.source() != &self.source
        {
            return Err(SphericalPresentationError::SourceMismatch {
                resource: "projection candidate",
            });
        }
        if !Arc::ptr_eq(self.map_presenter.layers_arc(), &self.layers)
            || !Arc::ptr_eq(self.globe_presenter.layers_arc(), &self.layers)
            || !Arc::ptr_eq(self.gpu_packet.layers_arc(), &self.layers)
        {
            return Err(SphericalPresentationError::FieldLayersNotShared);
        }
        if !self.layers.matches_camera_only_state(&self.state)
            || self.state.vector_view_zoom() != self.view_state.active_zoom()
        {
            return Err(SphericalPresentationError::ViewStateMismatch);
        }
        Ok(())
    }
}

/// A complete replacement for field layers and their two presenter bindings.
pub struct SphericalFieldCandidate {
    base: StateBoundCandidateBase,
    source: SphericalPresentationSource,
    map_presenter: SphericalMapPresenter,
    globe_presenter: SphericalGlobePresenter,
    layers: Arc<PreparedFieldLayers>,
    gpu_packet: Arc<SphericalGpuPacket>,
    view_state: SphericalPresentationViewState,
    state: SphericalFieldDisplayState,
    clock: DisplayRevisionClock,
}

impl SphericalFieldCandidate {
    #[cfg(test)]
    fn try_new(
        published: &PublishedSphericalPresentation,
        layers: Arc<PreparedFieldLayers>,
        state: SphericalFieldDisplayState,
        clock: DisplayRevisionClock,
    ) -> Result<Self, SphericalPresentationError> {
        Self::try_new_for_view(published, layers, *published.view_state(), state, clock)
    }

    fn try_new_for_view(
        published: &PublishedSphericalPresentation,
        layers: Arc<PreparedFieldLayers>,
        view_state: SphericalPresentationViewState,
        state: SphericalFieldDisplayState,
        clock: DisplayRevisionClock,
    ) -> Result<Self, SphericalPresentationError> {
        let base = StateBoundCandidateBase::from_published(published);
        if layers.source() != published.source() {
            return Err(SphericalPresentationError::SourceMismatch {
                resource: "field layers",
            });
        }
        if !layers.matches_camera_only_state(&state) {
            return Err(SphericalPresentationError::FieldStateMismatch);
        }
        let map_presenter =
            SphericalMapPresenter::try_new(Arc::clone(published.map_arc()), Arc::clone(&layers))?;
        let globe_presenter = SphericalGlobePresenter::try_new(
            Arc::clone(published.globe_arc()),
            Arc::clone(&layers),
        )?;
        let gpu_packet = Arc::new(SphericalGpuPacket::try_new(
            Arc::clone(published.map_arc()),
            published.gpu_packet().map_geometry_revision(),
            Arc::clone(published.globe_arc()),
            published.gpu_packet().globe_geometry_revision(),
            Arc::clone(&layers),
        )?);
        Ok(Self {
            base,
            source: published.source().clone(),
            map_presenter,
            globe_presenter,
            layers,
            gpu_packet,
            view_state,
            state,
            clock,
        })
    }

    fn validate(
        &self,
        published: &PublishedSphericalPresentation,
    ) -> Result<(), SphericalPresentationError> {
        if &self.source != published.source()
            || self.layers.source() != &self.source
            || self.gpu_packet.source() != &self.source
        {
            return Err(SphericalPresentationError::SourceMismatch {
                resource: "field candidate",
            });
        }
        if !Arc::ptr_eq(self.map_presenter.layers_arc(), &self.layers)
            || !Arc::ptr_eq(self.globe_presenter.layers_arc(), &self.layers)
            || !Arc::ptr_eq(self.gpu_packet.layers_arc(), &self.layers)
        {
            return Err(SphericalPresentationError::FieldLayersNotShared);
        }
        if self.map_presenter.map_arc().projection() != self.view_state.projection()
            || self.state.vector_view_zoom() != self.view_state.active_zoom()
        {
            return Err(SphericalPresentationError::ViewStateMismatch);
        }
        Ok(())
    }
}

/// Builds the exact eight external inputs accepted by the spherical natural graph.
pub fn build_spherical_external_artifacts(
    space: &SphericalSpaceSpec,
    formation: &WorldFormationSpec,
    tectonic: &TectonicSpec,
    geologic: &GeologicSpec,
) -> Result<ExternalArtifacts, SphericalPresentationError> {
    space.validate()?;
    formation.validate()?;
    tectonic.validate()?;
    geologic.validate()?;
    let mut external = ExternalArtifacts::new();
    external.insert(SphericalSpaceArtifact::new(space.clone()))?;
    external.insert(TectonicSpecArtifact::new(tectonic.clone()))?;
    external.insert(GeologicSpecArtifact::new(geologic.clone()))?;
    external.insert(ClimateSpecArtifact::new(Default::default()))?;
    external.insert(HydroErosionSpecArtifact::new(Default::default()))?;
    external.insert(WorldFormationSpecArtifact::new(formation.clone()))?;
    external.insert(RulePackSetArtifact::new(default_rule_pack_set()?))?;
    external.insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))?;
    Ok(external)
}

/// Runs only the formal spherical graph and constructs every publication derivative locally.
///
/// The returned standalone candidate is accepted by [`PublishedSphericalPresentation::try_new`].
/// Whole replacement candidates must instead be prepared from the current publication so their
/// revision and state lineage cannot be forged or restarted.
#[allow(clippy::too_many_arguments)]
pub fn build_spherical_presentation_candidate(
    root_seed: RootSeed,
    space: &SphericalSpaceSpec,
    formation: &WorldFormationSpec,
    tectonic: &TectonicSpec,
    geologic: &GeologicSpec,
    cache: &mut MemoryStageCache,
    current_state: &SphericalFieldDisplayState,
    clock: &DisplayRevisionClock,
) -> Result<SphericalPresentationCandidate, SphericalPresentationError> {
    build_spherical_presentation_candidate_for_view(
        root_seed,
        space,
        formation,
        tectonic,
        geologic,
        cache,
        SphericalPresentationViewState::default(),
        current_state,
        clock,
    )
}

/// Runs the formal graph and binds every derivative to an explicit complete view snapshot.
#[allow(clippy::too_many_arguments)]
pub fn build_spherical_presentation_candidate_for_view(
    root_seed: RootSeed,
    space: &SphericalSpaceSpec,
    formation: &WorldFormationSpec,
    tectonic: &TectonicSpec,
    geologic: &GeologicSpec,
    cache: &mut MemoryStageCache,
    view_state: SphericalPresentationViewState,
    current_state: &SphericalFieldDisplayState,
    clock: &DisplayRevisionClock,
) -> Result<SphericalPresentationCandidate, SphericalPresentationError> {
    build_spherical_presentation_candidate_impl(
        root_seed,
        space,
        formation,
        tectonic,
        geologic,
        cache,
        view_state,
        current_state,
        clock,
        FailureInjector::NONE,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_spherical_presentation_candidate_impl(
    root_seed: RootSeed,
    space: &SphericalSpaceSpec,
    formation: &WorldFormationSpec,
    tectonic: &TectonicSpec,
    geologic: &GeologicSpec,
    cache: &mut MemoryStageCache,
    view_state: SphericalPresentationViewState,
    current_state: &SphericalFieldDisplayState,
    clock: &DisplayRevisionClock,
    failure: FailureInjector,
) -> Result<SphericalPresentationCandidate, SphericalPresentationError> {
    build_spherical_presentation_candidate_impl_with_lineage(
        root_seed,
        space,
        formation,
        tectonic,
        geologic,
        cache,
        view_state,
        current_state,
        clock,
        failure,
        WorldCandidateLineage::NoBase,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_spherical_presentation_candidate_impl_with_lineage(
    root_seed: RootSeed,
    space: &SphericalSpaceSpec,
    formation: &WorldFormationSpec,
    tectonic: &TectonicSpec,
    geologic: &GeologicSpec,
    cache: &mut MemoryStageCache,
    view_state: SphericalPresentationViewState,
    current_state: &SphericalFieldDisplayState,
    clock: &DisplayRevisionClock,
    failure: FailureInjector,
    lineage: WorldCandidateLineage,
) -> Result<SphericalPresentationCandidate, SphericalPresentationError> {
    let external = build_spherical_external_artifacts(space, formation, tectonic, geologic)?;
    let outcome = BuildEngine::new(spherical_natural_foundation_graph()?)
        .build(root_seed, external, cache)?;
    failure.check("document")?;
    let document = Arc::new(SphericalNaturalFieldDocument::from_build_outcome(&outcome)?);
    let source = document.presentation_source();

    failure.check("locator")?;
    let locator = Arc::new(SphericalEntityLocator::new(
        source.clone(),
        document.surface.snapshot(),
    )?);

    failure.check("map")?;
    let projection = view_state.projection();
    let map = Arc::new(PreparedProjectedMap::build(
        source.clone(),
        document.surface.snapshot(),
        projection,
        SphericalMeshBudgets::DEFAULT,
    )?);

    failure.check("globe")?;
    let globe = Arc::new(PreparedGlobeMesh::build(
        source.clone(),
        document.surface.snapshot(),
        SphericalMeshBudgets::DEFAULT,
    )?);

    let mut next_state = current_state.clone();
    let mut next_clock = clock.clone();
    let map_geometry_revision = next_clock.issue()?;
    let globe_geometry_revision = next_clock.issue()?;
    failure.check("layers")?;
    let layers = Arc::new(prepare_spherical_document_layers(
        document.as_ref(),
        view_state.mode(),
        view_state.projection().kind(),
        view_state.map_camera(),
        view_state.globe_camera(),
        &mut next_state,
        &mut next_clock,
    )?);

    let map_presenter = SphericalMapPresenter::try_new(Arc::clone(&map), Arc::clone(&layers))?;
    let globe_presenter =
        SphericalGlobePresenter::try_new(Arc::clone(&globe), Arc::clone(&layers))?;
    validate_complete_source_set_without_gpu(&document, &source, &locator, &map, &globe, &layers)?;

    let gpu_packet = Arc::new(SphericalGpuPacket::try_new(
        map,
        map_geometry_revision,
        globe,
        globe_geometry_revision,
        Arc::clone(&layers),
    )?);
    let candidate = SphericalPresentationCandidate {
        lineage,
        document,
        source,
        locator,
        map_presenter,
        globe_presenter,
        layers,
        gpu_packet,
        view_state,
        state: next_state,
        clock: next_clock,
        report: outcome.report,
    };
    candidate.validate()?;
    Ok(candidate)
}

fn validate_complete_source_set(
    document: &SphericalNaturalFieldDocument,
    source: &SphericalPresentationSource,
    locator: &SphericalEntityLocator,
    map: &PreparedProjectedMap,
    globe: &PreparedGlobeMesh,
    layers: &PreparedFieldLayers,
    gpu_packet: &SphericalGpuPacket,
) -> Result<(), SphericalPresentationError> {
    validate_complete_source_set_without_gpu(document, source, locator, map, globe, layers)?;
    if gpu_packet.source() != source {
        return Err(SphericalPresentationError::SourceMismatch {
            resource: "GPU packet",
        });
    }
    if gpu_packet.map().source() != source || gpu_packet.globe().source() != source {
        return Err(SphericalPresentationError::SourceMismatch {
            resource: "GPU geometry",
        });
    }
    Ok(())
}

fn validate_complete_source_set_without_gpu(
    document: &SphericalNaturalFieldDocument,
    source: &SphericalPresentationSource,
    locator: &SphericalEntityLocator,
    map: &PreparedProjectedMap,
    globe: &PreparedGlobeMesh,
    layers: &PreparedFieldLayers,
) -> Result<(), SphericalPresentationError> {
    for (resource, matches) in [
        ("document", document.presentation_source() == *source),
        ("locator", locator.source() == source),
        ("projected map", map.source() == source),
        ("unit globe", globe.source() == source),
        ("field layers", layers.source() == source),
    ] {
        if !matches {
            return Err(SphericalPresentationError::SourceMismatch { resource });
        }
    }
    let cells = source.surface_ref().cell_count() as usize;
    let edges = source.surface_ref().edge_count() as usize;
    validate_cardinality("projected map", cells, map.cell_count())?;
    validate_cardinality("unit globe", cells, globe.cell_count())?;
    validate_cardinality("fill field", cells, layers.fill().len())?;
    validate_cardinality("diagnostics", cells, layers.diagnostics().len())?;
    if let Some(overlay) = layers.overlay() {
        use crate::view::PreparedSphericalOverlay;
        match overlay {
            PreparedSphericalOverlay::Edge(field) => {
                validate_cardinality("edge overlay", edges, field.len())?
            }
            PreparedSphericalOverlay::Vector(field) => {
                validate_cardinality("vector overlay", cells, field.len())?
            }
        }
    }
    Ok(())
}

fn validate_cardinality(
    resource: &'static str,
    expected: usize,
    actual: usize,
) -> Result<(), SphericalPresentationError> {
    if expected != actual {
        return Err(SphericalPresentationError::CardinalityMismatch {
            resource,
            expected,
            actual,
        });
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct FailureInjector {
    #[cfg(test)]
    boundary: Option<&'static str>,
}

impl FailureInjector {
    const NONE: Self = Self {
        #[cfg(test)]
        boundary: None,
    };

    #[cfg(test)]
    const fn at(boundary: &'static str) -> Self {
        Self {
            boundary: Some(boundary),
        }
    }

    fn check(self, boundary: &'static str) -> Result<(), SphericalPresentationError> {
        #[cfg(test)]
        if self.boundary == Some(boundary) {
            return Err(SphericalPresentationError::InjectedFailure { boundary });
        }
        let _ = boundary;
        Ok(())
    }
}

/// Structured failures from spherical composition or candidate validation.
#[derive(Debug, Error)]
pub enum SphericalPresentationError {
    #[error(transparent)]
    SphericalSpec(#[from] SphericalSpecError),
    #[error(transparent)]
    TectonicSpec(#[from] NaturalSpecError),
    #[error(transparent)]
    FormationSpec(#[from] WorldFormationSpecError),
    #[error(transparent)]
    GeologicSpec(#[from] GeologicSpecError),
    #[error(transparent)]
    BuiltinRules(#[from] BuiltinRuleError),
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
    #[error(transparent)]
    Graph(#[from] GraphError),
    #[error(transparent)]
    Build(#[from] BuildFailure),
    #[error(transparent)]
    Document(#[from] SphericalNaturalDisplayError),
    #[error(transparent)]
    Picking(#[from] SphericalPickingError),
    #[error(transparent)]
    Projection(#[from] SphericalProjectionError),
    #[error(transparent)]
    Mesh(#[from] SphericalMeshError),
    #[error(transparent)]
    Display(#[from] DisplayPrepareError),
    #[error(transparent)]
    Gpu(#[from] SphericalRenderError),
    #[error("{resource} has a different spherical presentation source")]
    SourceMismatch { resource: &'static str },
    #[error("{resource} cardinality {actual} does not match {expected}")]
    CardinalityMismatch {
        resource: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("map and globe presenters do not share one field-layer allocation")]
    FieldLayersNotShared,
    #[error("field candidate state does not match its prepared field layers")]
    FieldStateMismatch,
    #[error("spherical candidate view does not match its geometry, cameras, or active LOD")]
    ViewStateMismatch,
    #[error("initial spherical publication requires a standalone candidate")]
    InitialCandidateRequiresStandalone,
    #[error("{candidate} candidate was prepared from a stale spherical publication")]
    StaleCandidate { candidate: &'static str },
    #[cfg(test)]
    #[error("test-injected spherical candidate failure at {boundary}")]
    InjectedFailure { boundary: &'static str },
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        build_spherical_presentation_candidate, FailureInjector, PublishedSphericalPresentation,
        SphericalFieldCandidate, SphericalGpuPreparation, SphericalGpuPreparer,
        SphericalPresentationError,
    };
    use crate::engine::MemoryStageCache;
    use crate::view::{
        DisplayRevisionClock, GlobeCamera, MapCamera, SphericalFieldDisplayState,
        SphericalMeshBudgets, SphericalProjection, SphericalProjectionKind, SphericalViewMode,
    };
    use crate::world::natural::{
        plate_velocity_field_id, preliminary_prevailing_wind_m_s_field_id, GeologicSpec,
        TectonicSpec, WorldFormationSpec,
    };
    use crate::world::{Meters, RootSeed, SphericalSpaceSpec};

    #[derive(Default)]
    struct TestGpuPreparer {
        fail: bool,
        calls: usize,
        preflight_error: Option<crate::gpu::spherical::SphericalRenderError>,
    }

    impl SphericalGpuPreparer for TestGpuPreparer {
        fn preflight(
            &self,
            _preparation: SphericalGpuPreparation<'_>,
        ) -> Result<(), crate::gpu::spherical::SphericalRenderError> {
            match &self.preflight_error {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }

        fn prepare(
            &mut self,
            _preparation: SphericalGpuPreparation<'_>,
            _packet: &crate::gpu::spherical::SphericalGpuPacket,
        ) -> Result<(), crate::gpu::spherical::SphericalRenderError> {
            self.calls += 1;
            if self.fail {
                Err(crate::gpu::spherical::SphericalRenderError::InvalidViewport)
            } else {
                Ok(())
            }
        }
    }

    fn space() -> SphericalSpaceSpec {
        SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 162,
        }
    }

    #[test]
    fn renderer_ownership_preflight_precedes_candidate_validation_and_prepare() {
        let mut cache = MemoryStageCache::new();
        let mut invalid_initial = build_spherical_presentation_candidate(
            RootSeed::new(89),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let other = build_spherical_presentation_candidate(
            RootSeed::new(97),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        invalid_initial.source = other.source().clone();
        let mut initial_gpu = TestGpuPreparer {
            preflight_error: Some(
                crate::gpu::spherical::SphericalRenderError::RendererAlreadyInitialized,
            ),
            ..Default::default()
        };
        assert!(matches!(
            PublishedSphericalPresentation::try_new_with_preparer(
                invalid_initial,
                &mut initial_gpu
            ),
            Err(SphericalPresentationError::Gpu(
                crate::gpu::spherical::SphericalRenderError::RendererAlreadyInitialized
            ))
        ));
        assert_eq!(initial_gpu.calls, 0);

        let initial = build_spherical_presentation_candidate(
            RootSeed::new(101),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut replacement_gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(initial, &mut replacement_gpu)
                .unwrap();
        let mut invalid_replacement = published
            .prepare_replacement_candidate(
                RootSeed::new(103),
                &space(),
                &WorldFormationSpec::default(),
                &TectonicSpec::default(),
                &GeologicSpec::default(),
                &mut cache,
                published.state(),
            )
            .unwrap();
        invalid_replacement.source = other.source().clone();
        replacement_gpu.preflight_error =
            Some(crate::gpu::spherical::SphericalRenderError::RendererCurrentPacketMismatch);
        assert!(matches!(
            published.try_replace_with_preparer(invalid_replacement, &mut replacement_gpu),
            Err(SphericalPresentationError::Gpu(
                crate::gpu::spherical::SphericalRenderError::RendererCurrentPacketMismatch
            ))
        ));
        assert_eq!(replacement_gpu.calls, 1);
    }

    #[test]
    fn every_candidate_boundary_failure_preserves_the_complete_publication() {
        let mut cache = MemoryStageCache::new();
        let first = build_spherical_presentation_candidate(
            RootSeed::new(101),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(first, &mut gpu).unwrap();

        for (index, point) in ["document", "locator", "map", "globe", "layers", "gpu"]
            .into_iter()
            .enumerate()
        {
            let document = Arc::clone(published.document_arc());
            let locator = Arc::clone(published.locator_arc());
            let map = Arc::clone(published.map_arc());
            let globe = Arc::clone(published.globe_arc());
            let layers = Arc::clone(published.layers_arc());
            let packet = Arc::clone(published.gpu_packet_arc());
            let state = published.state().clone();
            let report = published.report().clone();
            let revisions = published.revisions();
            let mut expected_clock = published.clock().clone();
            let expected_next = expected_clock.issue().unwrap();

            let requested_state = published.state().clone();
            let candidate = published.prepare_replacement_candidate_impl(
                RootSeed::new(102 + index as u64),
                &space(),
                &WorldFormationSpec::default(),
                &TectonicSpec::default(),
                &GeologicSpec::default(),
                &mut cache,
                &requested_state,
                if point == "gpu" {
                    FailureInjector::NONE
                } else {
                    FailureInjector::at(point)
                },
            );
            let attempt = match candidate {
                Ok(candidate) => {
                    gpu.fail = point == "gpu";
                    let result = published.try_replace_with_preparer(candidate, &mut gpu);
                    gpu.fail = false;
                    result
                }
                Err(error) => Err(error),
            };
            if point == "gpu" {
                assert!(matches!(
                    attempt,
                    Err(SphericalPresentationError::Gpu(
                        crate::gpu::spherical::SphericalRenderError::InvalidViewport
                    ))
                ));
            } else {
                assert!(matches!(
                    attempt,
                    Err(SphericalPresentationError::InjectedFailure { boundary }) if boundary == point
                ));
            }

            assert!(Arc::ptr_eq(&document, published.document_arc()));
            assert!(Arc::ptr_eq(&locator, published.locator_arc()));
            assert!(Arc::ptr_eq(&map, published.map_arc()));
            assert!(Arc::ptr_eq(&globe, published.globe_arc()));
            assert!(Arc::ptr_eq(&layers, published.layers_arc()));
            assert!(Arc::ptr_eq(&packet, published.gpu_packet_arc()));
            assert_eq!(&state, published.state());
            assert_eq!(&report, published.report());
            assert_eq!(revisions, published.revisions());
            let mut actual_clock = published.clock().clone();
            assert_eq!(expected_next, actual_clock.issue().unwrap());
        }
    }

    #[test]
    fn same_source_layers_cannot_be_paired_with_mismatched_field_state() {
        let mut cache = MemoryStageCache::new();
        let candidate = build_spherical_presentation_candidate(
            RootSeed::new(211),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let published =
            PublishedSphericalPresentation::try_new_with_preparer(candidate, &mut gpu).unwrap();
        let mut mismatched_state = published.state().clone();
        mismatched_state.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));

        assert!(matches!(
            SphericalFieldCandidate::try_new(
                &published,
                Arc::clone(published.layers_arc()),
                mismatched_state,
                published.clock().clone(),
            ),
            Err(SphericalPresentationError::FieldStateMismatch)
        ));
    }

    #[test]
    fn smaller_candidate_gpu_failures_preserve_the_complete_publication() {
        let mut cache = MemoryStageCache::new();
        let candidate = build_spherical_presentation_candidate(
            RootSeed::new(223),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(candidate, &mut gpu).unwrap();

        let projection =
            SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.75).unwrap();
        let projection_candidate = published
            .prepare_projection_candidate(projection, SphericalMeshBudgets::DEFAULT)
            .unwrap();
        let document = Arc::clone(published.document_arc());
        let locator = Arc::clone(published.locator_arc());
        let map = Arc::clone(published.map_arc());
        let globe = Arc::clone(published.globe_arc());
        let layers = Arc::clone(published.layers_arc());
        let packet = Arc::clone(published.gpu_packet_arc());
        let state = published.state().clone();
        let report = published.report().clone();
        let revisions = published.revisions();
        let mut expected_clock = published.clock().clone();
        let expected_next = expected_clock.issue().unwrap();

        gpu.fail = true;
        assert!(matches!(
            published
                .try_replace_projection_candidate_with_preparer(projection_candidate, &mut gpu),
            Err(SphericalPresentationError::Gpu(
                crate::gpu::spherical::SphericalRenderError::InvalidViewport
            ))
        ));
        gpu.fail = false;

        assert!(Arc::ptr_eq(&document, published.document_arc()));
        assert!(Arc::ptr_eq(&locator, published.locator_arc()));
        assert!(Arc::ptr_eq(&map, published.map_arc()));
        assert!(Arc::ptr_eq(&globe, published.globe_arc()));
        assert!(Arc::ptr_eq(&layers, published.layers_arc()));
        assert!(Arc::ptr_eq(&packet, published.gpu_packet_arc()));
        assert_eq!(&state, published.state());
        assert_eq!(&report, published.report());
        assert_eq!(revisions, published.revisions());
        let mut actual_clock = published.clock().clone();
        assert_eq!(expected_next, actual_clock.issue().unwrap());

        let mut requested_state = published.state().clone();
        requested_state.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
        let field_candidate = published
            .prepare_field_candidate(
                requested_state,
                SphericalViewMode::Map,
                MapCamera::default(),
                GlobeCamera::default(),
            )
            .unwrap();

        gpu.fail = true;
        assert!(matches!(
            published.try_replace_field_candidate_with_preparer(field_candidate, &mut gpu),
            Err(SphericalPresentationError::Gpu(
                crate::gpu::spherical::SphericalRenderError::InvalidViewport
            ))
        ));

        assert!(Arc::ptr_eq(&document, published.document_arc()));
        assert!(Arc::ptr_eq(&locator, published.locator_arc()));
        assert!(Arc::ptr_eq(&map, published.map_arc()));
        assert!(Arc::ptr_eq(&globe, published.globe_arc()));
        assert!(Arc::ptr_eq(&layers, published.layers_arc()));
        assert!(Arc::ptr_eq(&packet, published.gpu_packet_arc()));
        assert_eq!(&state, published.state());
        assert_eq!(&report, published.report());
        assert_eq!(revisions, published.revisions());
        let mut actual_clock = published.clock().clone();
        assert_eq!(expected_next, actual_clock.issue().unwrap());
    }

    #[test]
    fn stale_projection_candidate_is_rejected_before_gpu_and_preserves_newer_projection() {
        let mut cache = MemoryStageCache::new();
        let candidate = build_spherical_presentation_candidate(
            RootSeed::new(227),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(candidate, &mut gpu).unwrap();
        let projection_a = published
            .prepare_projection_candidate(
                SphericalProjection::new(SphericalProjectionKind::Equirectangular, -0.5).unwrap(),
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap();
        let projection_b = published
            .prepare_projection_candidate(
                SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.75).unwrap(),
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap();
        published
            .try_replace_projection_candidate_with_preparer(projection_b, &mut gpu)
            .unwrap();

        let document = Arc::clone(published.document_arc());
        let locator = Arc::clone(published.locator_arc());
        let map = Arc::clone(published.map_arc());
        let globe = Arc::clone(published.globe_arc());
        let layers = Arc::clone(published.layers_arc());
        let packet = Arc::clone(published.gpu_packet_arc());
        let state = published.state().clone();
        let report = published.report().clone();
        let revisions = published.revisions();
        let mut expected_clock = published.clock().clone();
        let expected_next = expected_clock.issue().unwrap();
        let prepare_calls = gpu.calls;

        let attempt =
            published.try_replace_projection_candidate_with_preparer(projection_a, &mut gpu);

        assert!(matches!(
            attempt,
            Err(SphericalPresentationError::StaleCandidate {
                candidate: "projection"
            })
        ));
        assert_eq!(gpu.calls, prepare_calls);
        assert!(Arc::ptr_eq(&document, published.document_arc()));
        assert!(Arc::ptr_eq(&locator, published.locator_arc()));
        assert!(Arc::ptr_eq(&map, published.map_arc()));
        assert!(Arc::ptr_eq(&globe, published.globe_arc()));
        assert!(Arc::ptr_eq(&layers, published.layers_arc()));
        assert!(Arc::ptr_eq(&packet, published.gpu_packet_arc()));
        assert_eq!(&state, published.state());
        assert_eq!(&report, published.report());
        assert_eq!(revisions, published.revisions());
        let mut actual_clock = published.clock().clone();
        assert_eq!(expected_next, actual_clock.issue().unwrap());
    }

    #[test]
    fn stale_field_candidate_is_rejected_before_gpu_and_preserves_newer_fields() {
        let mut cache = MemoryStageCache::new();
        let candidate = build_spherical_presentation_candidate(
            RootSeed::new(229),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(candidate, &mut gpu).unwrap();
        let mut state_a = published.state().clone();
        state_a.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
        let mut state_b = published.state().clone();
        state_b.select_overlay(Some(plate_velocity_field_id()));
        let field_a = published
            .prepare_field_candidate(
                state_a,
                SphericalViewMode::Map,
                MapCamera::default(),
                GlobeCamera::default(),
            )
            .unwrap();
        let field_b = published
            .prepare_field_candidate(
                state_b,
                SphericalViewMode::Map,
                MapCamera::default(),
                GlobeCamera::default(),
            )
            .unwrap();
        assert_eq!(
            field_a.gpu_packet.map_geometry_revision(),
            field_b.gpu_packet.map_geometry_revision()
        );
        assert_eq!(
            field_a.gpu_packet.globe_geometry_revision(),
            field_b.gpu_packet.globe_geometry_revision()
        );
        assert_eq!(field_a.layers.revisions(), field_b.layers.revisions());
        published
            .try_replace_field_candidate_with_preparer(field_b, &mut gpu)
            .unwrap();

        let document = Arc::clone(published.document_arc());
        let locator = Arc::clone(published.locator_arc());
        let map = Arc::clone(published.map_arc());
        let globe = Arc::clone(published.globe_arc());
        let layers = Arc::clone(published.layers_arc());
        let packet = Arc::clone(published.gpu_packet_arc());
        let state = published.state().clone();
        let report = published.report().clone();
        let revisions = published.revisions();
        let mut expected_clock = published.clock().clone();
        let expected_next = expected_clock.issue().unwrap();
        let prepare_calls = gpu.calls;

        let attempt = published.try_replace_field_candidate_with_preparer(field_a, &mut gpu);

        assert!(matches!(
            attempt,
            Err(SphericalPresentationError::StaleCandidate { candidate: "field" })
        ));
        assert_eq!(gpu.calls, prepare_calls);
        assert!(Arc::ptr_eq(&document, published.document_arc()));
        assert!(Arc::ptr_eq(&locator, published.locator_arc()));
        assert!(Arc::ptr_eq(&map, published.map_arc()));
        assert!(Arc::ptr_eq(&globe, published.globe_arc()));
        assert!(Arc::ptr_eq(&layers, published.layers_arc()));
        assert!(Arc::ptr_eq(&packet, published.gpu_packet_arc()));
        assert_eq!(&state, published.state());
        assert_eq!(&report, published.report());
        assert_eq!(revisions, published.revisions());
        let mut actual_clock = published.clock().clone();
        assert_eq!(expected_next, actual_clock.issue().unwrap());
    }

    #[test]
    fn field_candidate_cannot_roll_back_a_newer_projection() {
        let mut cache = MemoryStageCache::new();
        let candidate = build_spherical_presentation_candidate(
            RootSeed::new(233),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(candidate, &mut gpu).unwrap();
        let mut requested_state = published.state().clone();
        requested_state.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
        let field = published
            .prepare_field_candidate(
                requested_state,
                SphericalViewMode::Map,
                MapCamera::default(),
                GlobeCamera::default(),
            )
            .unwrap();
        let projection = published
            .prepare_projection_candidate(
                SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.25).unwrap(),
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap();
        published
            .try_replace_projection_candidate_with_preparer(projection, &mut gpu)
            .unwrap();

        let map = Arc::clone(published.map_arc());
        let packet = Arc::clone(published.gpu_packet_arc());
        let layers = Arc::clone(published.layers_arc());
        let state = published.state().clone();
        let revisions = published.revisions();
        let mut expected_clock = published.clock().clone();
        let expected_next = expected_clock.issue().unwrap();
        let prepare_calls = gpu.calls;

        let attempt = published.try_replace_field_candidate_with_preparer(field, &mut gpu);

        assert!(matches!(
            attempt,
            Err(SphericalPresentationError::StaleCandidate { candidate: "field" })
        ));
        assert_eq!(gpu.calls, prepare_calls);
        assert!(Arc::ptr_eq(&map, published.map_arc()));
        assert!(Arc::ptr_eq(&packet, published.gpu_packet_arc()));
        assert!(Arc::ptr_eq(&layers, published.layers_arc()));
        assert_eq!(&state, published.state());
        assert_eq!(revisions, published.revisions());
        let mut actual_clock = published.clock().clone();
        assert_eq!(expected_next, actual_clock.issue().unwrap());
    }

    #[test]
    fn projection_candidate_cannot_overwrite_newer_field_layers() {
        let mut cache = MemoryStageCache::new();
        let candidate = build_spherical_presentation_candidate(
            RootSeed::new(239),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(candidate, &mut gpu).unwrap();
        let projection = published
            .prepare_projection_candidate(
                SphericalProjection::new(SphericalProjectionKind::Equirectangular, -0.25).unwrap(),
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap();
        let mut requested_state = published.state().clone();
        requested_state.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
        let field = published
            .prepare_field_candidate(
                requested_state,
                SphericalViewMode::Map,
                MapCamera::default(),
                GlobeCamera::default(),
            )
            .unwrap();
        published
            .try_replace_field_candidate_with_preparer(field, &mut gpu)
            .unwrap();

        let map = Arc::clone(published.map_arc());
        let packet = Arc::clone(published.gpu_packet_arc());
        let layers = Arc::clone(published.layers_arc());
        let state = published.state().clone();
        let revisions = published.revisions();
        let mut expected_clock = published.clock().clone();
        let expected_next = expected_clock.issue().unwrap();
        let prepare_calls = gpu.calls;

        let attempt =
            published.try_replace_projection_candidate_with_preparer(projection, &mut gpu);

        assert!(matches!(
            attempt,
            Err(SphericalPresentationError::StaleCandidate {
                candidate: "projection"
            })
        ));
        assert_eq!(gpu.calls, prepare_calls);
        assert!(Arc::ptr_eq(&map, published.map_arc()));
        assert!(Arc::ptr_eq(&packet, published.gpu_packet_arc()));
        assert!(Arc::ptr_eq(&layers, published.layers_arc()));
        assert_eq!(&state, published.state());
        assert_eq!(revisions, published.revisions());
        let mut actual_clock = published.clock().clone();
        assert_eq!(expected_next, actual_clock.issue().unwrap());
    }

    #[test]
    fn whole_candidates_require_the_exact_current_lineage() {
        let mut cache = MemoryStageCache::new();
        let initial = build_spherical_presentation_candidate(
            RootSeed::new(241),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(initial, &mut gpu).unwrap();
        let mut state_a = published.state().clone();
        state_a.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
        let mut state_b = published.state().clone();
        state_b.select_overlay(Some(plate_velocity_field_id()));
        let whole_a = published
            .prepare_replacement_candidate(
                RootSeed::new(241),
                &space(),
                &WorldFormationSpec::default(),
                &TectonicSpec::default(),
                &GeologicSpec::default(),
                &mut cache,
                &state_a,
            )
            .unwrap();
        let whole_b = published
            .prepare_replacement_candidate(
                RootSeed::new(241),
                &space(),
                &WorldFormationSpec::default(),
                &TectonicSpec::default(),
                &GeologicSpec::default(),
                &mut cache,
                &state_b,
            )
            .unwrap();
        assert_eq!(whole_a.revisions(), whole_b.revisions());
        assert_ne!(whole_a.state(), whole_b.state());
        published
            .try_replace_with_preparer(whole_b, &mut gpu)
            .unwrap();

        let document = Arc::clone(published.document_arc());
        let locator = Arc::clone(published.locator_arc());
        let map = Arc::clone(published.map_arc());
        let globe = Arc::clone(published.globe_arc());
        let layers = Arc::clone(published.layers_arc());
        let packet = Arc::clone(published.gpu_packet_arc());
        let state = published.state().clone();
        let report = published.report().clone();
        let revisions = published.revisions();
        let mut expected_clock = published.clock().clone();
        let expected_next = expected_clock.issue().unwrap();
        let prepare_calls = gpu.calls;

        assert!(matches!(
            published.try_replace_with_preparer(whole_a, &mut gpu),
            Err(SphericalPresentationError::StaleCandidate { candidate: "world" })
        ));
        assert_eq!(gpu.calls, prepare_calls);

        let standalone_default_clock = build_spherical_presentation_candidate(
            RootSeed::new(241),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &state_a,
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        assert_eq!(standalone_default_clock.source(), published.source());
        assert!(matches!(
            published.try_replace_with_preparer(standalone_default_clock, &mut gpu),
            Err(SphericalPresentationError::StaleCandidate { candidate: "world" })
        ));
        assert_eq!(gpu.calls, prepare_calls);
        assert!(Arc::ptr_eq(&document, published.document_arc()));
        assert!(Arc::ptr_eq(&locator, published.locator_arc()));
        assert!(Arc::ptr_eq(&map, published.map_arc()));
        assert!(Arc::ptr_eq(&globe, published.globe_arc()));
        assert!(Arc::ptr_eq(&layers, published.layers_arc()));
        assert!(Arc::ptr_eq(&packet, published.gpu_packet_arc()));
        assert_eq!(&state, published.state());
        assert_eq!(&report, published.report());
        assert_eq!(revisions, published.revisions());
        let mut actual_clock = published.clock().clone();
        assert_eq!(expected_next, actual_clock.issue().unwrap());
    }

    #[test]
    fn fresh_different_seed_whole_candidate_continues_revision_lineage() {
        let mut cache = MemoryStageCache::new();
        let initial = build_spherical_presentation_candidate(
            RootSeed::new(251),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(initial, &mut gpu).unwrap();
        let projection = published
            .prepare_projection_candidate(
                SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.5).unwrap(),
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap();
        published
            .try_replace_projection_candidate_with_preparer(projection, &mut gpu)
            .unwrap();
        let old_document = Arc::clone(published.document_arc());
        let old_locator = Arc::clone(published.locator_arc());
        let old_map = Arc::clone(published.map_arc());
        let old_globe = Arc::clone(published.globe_arc());
        let old_layers = Arc::clone(published.layers_arc());
        let old_packet = Arc::clone(published.gpu_packet_arc());
        let old_revisions = published.revisions();
        let old_max_revision = [
            old_revisions.0.get(),
            old_revisions.1.get(),
            old_revisions.2.fill.get(),
            old_revisions.2.overlay.get(),
            old_revisions.2.diagnostics.get(),
            old_revisions.2.fill_palette.get(),
            old_revisions.2.overlay_palette.get(),
            old_revisions.2.vector_glyphs.get(),
        ]
        .into_iter()
        .max()
        .unwrap();
        let mut old_clock = published.clock().clone();
        let old_next = old_clock.issue().unwrap();

        let fresh = published
            .prepare_replacement_candidate(
                RootSeed::new(257),
                &space(),
                &WorldFormationSpec::default(),
                &TectonicSpec::default(),
                &GeologicSpec::default(),
                &mut cache,
                published.state(),
            )
            .unwrap();
        let fresh_revisions = fresh.revisions();
        assert!([
            fresh_revisions.0.get(),
            fresh_revisions.1.get(),
            fresh_revisions.2.fill.get(),
            fresh_revisions.2.overlay.get(),
            fresh_revisions.2.diagnostics.get(),
            fresh_revisions.2.fill_palette.get(),
            fresh_revisions.2.overlay_palette.get(),
            fresh_revisions.2.vector_glyphs.get(),
        ]
        .into_iter()
        .all(|revision| revision > old_max_revision));

        published
            .try_replace_with_preparer(fresh, &mut gpu)
            .unwrap();

        assert!(!Arc::ptr_eq(&old_document, published.document_arc()));
        assert!(!Arc::ptr_eq(&old_locator, published.locator_arc()));
        assert!(!Arc::ptr_eq(&old_map, published.map_arc()));
        assert!(!Arc::ptr_eq(&old_globe, published.globe_arc()));
        assert!(!Arc::ptr_eq(&old_layers, published.layers_arc()));
        assert!(!Arc::ptr_eq(&old_packet, published.gpu_packet_arc()));
        assert_eq!(published.source().root_seed(), RootSeed::new(257));
        let mut new_clock = published.clock().clone();
        assert!(new_clock.issue().unwrap() > old_next);
    }

    #[test]
    fn projection_candidate_preserves_newer_uniform_only_state() {
        let mut cache = MemoryStageCache::new();
        let initial = build_spherical_presentation_candidate(
            RootSeed::new(263),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(initial, &mut gpu).unwrap();
        let old_map = Arc::clone(published.map_arc());
        let projection = published
            .prepare_projection_candidate(
                SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.125).unwrap(),
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap();

        published.current.state.set_vector_paused(true);
        published
            .try_replace_projection_candidate_with_preparer(projection, &mut gpu)
            .unwrap();

        assert!(published.state().vector_paused());
        assert!(!Arc::ptr_eq(&old_map, published.map_arc()));
    }

    #[test]
    fn successful_whole_replacement_retires_predecessor_packet_and_caches() {
        let mut cache = MemoryStageCache::new();
        let initial = build_spherical_presentation_candidate(
            RootSeed::new(269),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(initial, &mut gpu).unwrap();
        let old_packet = Arc::downgrade(published.gpu_packet_arc());
        let old_map = Arc::downgrade(published.map_arc());
        let old_globe = Arc::downgrade(published.globe_arc());
        let old_layers = Arc::downgrade(published.layers_arc());
        let requested_state = published.state().clone();
        let replacement = published
            .prepare_replacement_candidate(
                RootSeed::new(271),
                &space(),
                &WorldFormationSpec::default(),
                &TectonicSpec::default(),
                &GeologicSpec::default(),
                &mut cache,
                &requested_state,
            )
            .unwrap();

        published
            .try_replace_with_preparer(replacement, &mut gpu)
            .unwrap();

        assert!(old_packet.upgrade().is_none());
        assert!(old_map.upgrade().is_none());
        assert!(old_globe.upgrade().is_none());
        assert!(old_layers.upgrade().is_none());
        assert!(matches!(
            &published.current.lineage,
            super::WorldCandidateLineage::NoBase
        ));
        assert_eq!(published.source().root_seed(), RootSeed::new(271));
        assert_eq!(published.map().source(), published.source());
        assert_eq!(published.globe().source(), published.source());
        assert_eq!(published.layers().source(), published.source());
    }

    #[test]
    fn smaller_publications_do_not_resurrect_retired_whole_lineage() {
        let mut cache = MemoryStageCache::new();
        let initial = build_spherical_presentation_candidate(
            RootSeed::new(277),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(initial, &mut gpu).unwrap();
        let old_packet = Arc::downgrade(published.gpu_packet_arc());
        let old_map = Arc::downgrade(published.map_arc());
        let old_globe = Arc::downgrade(published.globe_arc());
        let old_layers = Arc::downgrade(published.layers_arc());
        let requested_state = published.state().clone();
        let replacement = published
            .prepare_replacement_candidate(
                RootSeed::new(281),
                &space(),
                &WorldFormationSpec::default(),
                &TectonicSpec::default(),
                &GeologicSpec::default(),
                &mut cache,
                &requested_state,
            )
            .unwrap();
        published
            .try_replace_with_preparer(replacement, &mut gpu)
            .unwrap();
        let projection = published
            .prepare_projection_candidate(
                SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.375).unwrap(),
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap();
        published
            .try_replace_projection_candidate_with_preparer(projection, &mut gpu)
            .unwrap();
        let mut field_state = published.state().clone();
        field_state.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
        let field = published
            .prepare_field_candidate(
                field_state,
                SphericalViewMode::Map,
                MapCamera::default(),
                GlobeCamera::default(),
            )
            .unwrap();
        published
            .try_replace_field_candidate_with_preparer(field, &mut gpu)
            .unwrap();

        assert!(old_packet.upgrade().is_none());
        assert!(old_map.upgrade().is_none());
        assert!(old_globe.upgrade().is_none());
        assert!(old_layers.upgrade().is_none());
        assert!(matches!(
            &published.current.lineage,
            super::WorldCandidateLineage::NoBase
        ));
        assert_eq!(published.source().root_seed(), RootSeed::new(281));
    }

    #[test]
    fn failed_whole_replacement_keeps_predecessor_live_and_current() {
        let mut cache = MemoryStageCache::new();
        let initial = build_spherical_presentation_candidate(
            RootSeed::new(283),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let mut published =
            PublishedSphericalPresentation::try_new_with_preparer(initial, &mut gpu).unwrap();
        let old_packet = Arc::downgrade(published.gpu_packet_arc());
        let old_map = Arc::downgrade(published.map_arc());
        let old_globe = Arc::downgrade(published.globe_arc());
        let old_layers = Arc::downgrade(published.layers_arc());
        let requested_state = published.state().clone();
        let replacement = published
            .prepare_replacement_candidate(
                RootSeed::new(293),
                &space(),
                &WorldFormationSpec::default(),
                &TectonicSpec::default(),
                &GeologicSpec::default(),
                &mut cache,
                &requested_state,
            )
            .unwrap();

        gpu.fail = true;
        assert!(matches!(
            published.try_replace_with_preparer(replacement, &mut gpu),
            Err(SphericalPresentationError::Gpu(
                crate::gpu::spherical::SphericalRenderError::InvalidViewport
            ))
        ));

        assert!(Arc::ptr_eq(
            &old_packet.upgrade().unwrap(),
            published.gpu_packet_arc()
        ));
        assert!(Arc::ptr_eq(
            &old_map.upgrade().unwrap(),
            published.map_arc()
        ));
        assert!(Arc::ptr_eq(
            &old_globe.upgrade().unwrap(),
            published.globe_arc()
        ));
        assert!(Arc::ptr_eq(
            &old_layers.upgrade().unwrap(),
            published.layers_arc()
        ));
        assert_eq!(published.source().root_seed(), RootSeed::new(283));
    }

    #[test]
    fn initial_publication_rejects_predecessor_lineage_before_gpu_prepare() {
        let mut cache = MemoryStageCache::new();
        let initial = build_spherical_presentation_candidate(
            RootSeed::new(307),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        let mut gpu = TestGpuPreparer::default();
        let predecessor =
            PublishedSphericalPresentation::try_new_with_preparer(initial, &mut gpu).unwrap();
        let old_packet = Arc::downgrade(predecessor.gpu_packet_arc());
        let old_map = Arc::downgrade(predecessor.map_arc());
        let old_globe = Arc::downgrade(predecessor.globe_arc());
        let old_layers = Arc::downgrade(predecessor.layers_arc());
        let requested_state = predecessor.state().clone();
        let replacement = predecessor
            .prepare_replacement_candidate(
                RootSeed::new(311),
                &space(),
                &WorldFormationSpec::default(),
                &TectonicSpec::default(),
                &GeologicSpec::default(),
                &mut cache,
                &requested_state,
            )
            .unwrap();

        let calls_before = gpu.calls;
        let result = PublishedSphericalPresentation::try_new_with_preparer(replacement, &mut gpu);

        assert!(matches!(
            result,
            Err(SphericalPresentationError::InitialCandidateRequiresStandalone)
        ));
        assert_eq!(gpu.calls, calls_before);
        assert!(Arc::ptr_eq(
            &old_packet.upgrade().unwrap(),
            predecessor.gpu_packet_arc()
        ));
        assert!(Arc::ptr_eq(
            &old_map.upgrade().unwrap(),
            predecessor.map_arc()
        ));
        assert!(Arc::ptr_eq(
            &old_globe.upgrade().unwrap(),
            predecessor.globe_arc()
        ));
        assert!(Arc::ptr_eq(
            &old_layers.upgrade().unwrap(),
            predecessor.layers_arc()
        ));
        assert_eq!(predecessor.source().root_seed(), RootSeed::new(307));
    }
}
