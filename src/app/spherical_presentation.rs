//! Composition root for one complete source-bound spherical presentation.

use std::sync::Arc;

use eframe::egui_wgpu::wgpu;
use thiserror::Error;

use super::field_document::{prepare_spherical_document_layers, update_spherical_document_layers};
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
    SphericalPickingError, SphericalPresentationSource, SphericalProjection,
    SphericalProjectionError, SphericalProjectionKind, SphericalViewMode,
};
use crate::world::natural::{
    GeologicSpec, GeologicSpecError, NaturalSpecError, TectonicSpec, WorldFormationSpec,
    WorldFormationSpecError,
};
use crate::world::{RootSeed, SphericalSpaceSpec, SphericalSpecError};

/// The renderer-side preparation gate that must succeed before CPU publication changes.
trait SphericalGpuPreparer {
    /// Atomically prepares the complete GPU packet or retains the previous renderer state.
    fn prepare(&mut self, packet: &SphericalGpuPacket) -> Result<(), SphericalRenderError>;
}

/// Production adapter from publication orchestration to the spherical wgpu renderer.
pub struct SphericalRendererPreparer<'a> {
    renderer: &'a mut SphericalFieldRenderer,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
}

impl<'a> SphericalRendererPreparer<'a> {
    /// Binds the renderer and device resources used for one or more atomic publications.
    pub const fn new(
        renderer: &'a mut SphericalFieldRenderer,
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
    ) -> Self {
        Self {
            renderer,
            device,
            queue,
        }
    }
}

impl SphericalGpuPreparer for SphericalRendererPreparer<'_> {
    fn prepare(&mut self, packet: &SphericalGpuPacket) -> Result<(), SphericalRenderError> {
        self.renderer
            .prepare_packet(self.device, self.queue, packet)
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
#[derive(Clone)]
pub struct SphericalPresentationCandidate {
    document: Arc<SphericalNaturalFieldDocument>,
    source: SphericalPresentationSource,
    locator: Arc<SphericalEntityLocator>,
    map_presenter: SphericalMapPresenter,
    globe_presenter: SphericalGlobePresenter,
    layers: Arc<PreparedFieldLayers>,
    gpu_packet: Arc<SphericalGpuPacket>,
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
}

/// The currently published complete spherical world and all of its presentation derivatives.
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
        candidate: SphericalPresentationCandidate,
        gpu: &mut P,
    ) -> Result<Self, SphericalPresentationError> {
        candidate.validate()?;
        gpu.prepare(candidate.gpu_packet())?;
        Ok(Self { current: candidate })
    }

    /// Revalidates, prepares GPU resources, then replaces the whole publication once.
    pub fn try_replace(
        &mut self,
        candidate: SphericalPresentationCandidate,
        gpu: &mut SphericalRendererPreparer<'_>,
    ) -> Result<(), SphericalPresentationError> {
        self.try_replace_with_preparer(candidate, gpu)
    }

    fn try_replace_with_preparer<P: SphericalGpuPreparer + ?Sized>(
        &mut self,
        candidate: SphericalPresentationCandidate,
        gpu: &mut P,
    ) -> Result<(), SphericalPresentationError> {
        candidate.validate()?;
        gpu.prepare(candidate.gpu_packet())?;
        self.current = candidate;
        Ok(())
    }

    /// Builds a disposable projected-map candidate without changing the current map or clock.
    pub fn prepare_projection_candidate(
        &self,
        projection: SphericalProjection,
        budgets: SphericalMeshBudgets,
    ) -> Result<SphericalProjectionCandidate, SphericalPresentationError> {
        let map = Arc::new(PreparedProjectedMap::build(
            self.source().clone(),
            self.document().surface.snapshot(),
            projection,
            budgets,
        )?);
        let mut clock = self.clock().clone();
        let revision = clock.issue()?;
        SphericalProjectionCandidate::try_new(self, map, revision, clock)
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
        candidate.validate(self)?;
        let mut next = self.current.clone();
        next.map_presenter = candidate.map_presenter;
        next.gpu_packet = candidate.gpu_packet;
        next.clock = candidate.clock;
        next.validate()?;
        gpu.prepare(next.gpu_packet())?;
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
        let mut state = requested_state;
        let mut clock = self.clock().clone();
        let layers = Arc::new(update_spherical_document_layers(
            self.document(),
            self.layers(),
            mode,
            self.map().projection().kind(),
            map_camera,
            globe_camera,
            &mut state,
            &mut clock,
        )?);
        SphericalFieldCandidate::try_new(self, layers, state, clock)
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
        candidate.validate(self)?;
        let mut next = self.current.clone();
        next.map_presenter = candidate.map_presenter;
        next.globe_presenter = candidate.globe_presenter;
        next.layers = candidate.layers;
        next.gpu_packet = candidate.gpu_packet;
        next.state = candidate.state;
        next.clock = candidate.clock;
        next.validate()?;
        gpu.prepare(next.gpu_packet())?;
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

/// A complete replacement for only the projected-map sub-cache.
pub struct SphericalProjectionCandidate {
    source: SphericalPresentationSource,
    map_presenter: SphericalMapPresenter,
    gpu_packet: Arc<SphericalGpuPacket>,
    clock: DisplayRevisionClock,
}

impl SphericalProjectionCandidate {
    fn try_new(
        published: &PublishedSphericalPresentation,
        map: Arc<PreparedProjectedMap>,
        revision: DisplayRevision,
        clock: DisplayRevisionClock,
    ) -> Result<Self, SphericalPresentationError> {
        let source = published.source().clone();
        if map.source() != &source {
            return Err(SphericalPresentationError::SourceMismatch {
                resource: "projected map",
            });
        }
        let map_presenter =
            SphericalMapPresenter::try_new(Arc::clone(&map), Arc::clone(published.layers_arc()))?;
        let gpu_packet = Arc::new(SphericalGpuPacket::try_new(
            map,
            revision,
            Arc::clone(published.globe_arc()),
            published.gpu_packet().globe_geometry_revision(),
            Arc::clone(published.layers_arc()),
        )?);
        Ok(Self {
            source,
            map_presenter,
            gpu_packet,
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
        if self.map_presenter.map_arc().source() != &self.source
            || self.gpu_packet.source() != &self.source
        {
            return Err(SphericalPresentationError::SourceMismatch {
                resource: "projection candidate",
            });
        }
        if !Arc::ptr_eq(self.map_presenter.layers_arc(), published.layers_arc())
            || !Arc::ptr_eq(self.gpu_packet.layers_arc(), published.layers_arc())
        {
            return Err(SphericalPresentationError::FieldLayersNotShared);
        }
        Ok(())
    }
}

/// A complete replacement for field layers and their two presenter bindings.
pub struct SphericalFieldCandidate {
    source: SphericalPresentationSource,
    map_presenter: SphericalMapPresenter,
    globe_presenter: SphericalGlobePresenter,
    layers: Arc<PreparedFieldLayers>,
    gpu_packet: Arc<SphericalGpuPacket>,
    state: SphericalFieldDisplayState,
    clock: DisplayRevisionClock,
}

impl SphericalFieldCandidate {
    fn try_new(
        published: &PublishedSphericalPresentation,
        layers: Arc<PreparedFieldLayers>,
        state: SphericalFieldDisplayState,
        clock: DisplayRevisionClock,
    ) -> Result<Self, SphericalPresentationError> {
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
            source: published.source().clone(),
            map_presenter,
            globe_presenter,
            layers,
            gpu_packet,
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
    build_spherical_presentation_candidate_impl(
        root_seed,
        space,
        formation,
        tectonic,
        geologic,
        cache,
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
    current_state: &SphericalFieldDisplayState,
    clock: &DisplayRevisionClock,
    failure: FailureInjector,
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
    let projection = SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0)?;
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
        SphericalViewMode::Map,
        SphericalProjectionKind::EqualEarth,
        MapCamera::default(),
        GlobeCamera::default(),
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
        document,
        source,
        locator,
        map_presenter,
        globe_presenter,
        layers,
        gpu_packet,
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
    #[cfg(test)]
    #[error("test-injected spherical candidate failure at {boundary}")]
    InjectedFailure { boundary: &'static str },
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        build_spherical_presentation_candidate, build_spherical_presentation_candidate_impl,
        FailureInjector, PublishedSphericalPresentation, SphericalFieldCandidate,
        SphericalGpuPreparer, SphericalPresentationError,
    };
    use crate::engine::MemoryStageCache;
    use crate::view::{
        DisplayRevisionClock, GlobeCamera, MapCamera, SphericalFieldDisplayState,
        SphericalMeshBudgets, SphericalProjection, SphericalProjectionKind, SphericalViewMode,
    };
    use crate::world::natural::{
        preliminary_prevailing_wind_m_s_field_id, GeologicSpec, TectonicSpec, WorldFormationSpec,
    };
    use crate::world::{Meters, RootSeed, SphericalSpaceSpec};

    #[derive(Default)]
    struct TestGpuPreparer {
        fail: bool,
    }

    impl SphericalGpuPreparer for TestGpuPreparer {
        fn prepare(
            &mut self,
            _packet: &crate::gpu::spherical::SphericalGpuPacket,
        ) -> Result<(), crate::gpu::spherical::SphericalRenderError> {
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

            let candidate = build_spherical_presentation_candidate_impl(
                RootSeed::new(102 + index as u64),
                &space(),
                &WorldFormationSpec::default(),
                &TectonicSpec::default(),
                &GeologicSpec::default(),
                &mut cache,
                published.state(),
                published.clock(),
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
}
