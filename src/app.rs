use std::sync::Arc;

use eframe::egui_wgpu::RenderState;
use thiserror::Error;

mod amplified_mesh;
mod field_document;
mod frame_stats;
#[cfg_attr(not(test), allow(dead_code))]
mod legacy_display;
mod natural_display;
mod natural_field_payloads;
mod spherical_formation_display;
#[cfg_attr(not(test), allow(dead_code))]
mod spherical_natural_display;
mod spherical_presentation;

pub use spherical_formation_display::{
    FormationAreaSummary, SphericalFormationDisplayError, SphericalFormationFieldDocument,
};
pub use spherical_natural_display::{
    SphericalNaturalAreaSummary, SphericalNaturalDisplayError, SphericalNaturalFieldDocument,
};
pub use spherical_presentation::{
    build_spherical_external_artifacts, build_spherical_formation_candidate_for_view,
    build_spherical_formation_external_artifacts, build_spherical_presentation_candidate,
    build_spherical_presentation_candidate_for_view, run_spherical_world_build,
    PublishedSphericalPresentation, SphericalFieldCandidate, SphericalGlobePresenter,
    SphericalMapPresenter, SphericalPresentationCandidate, SphericalPresentationError,
    SphericalProjectionCandidate, SphericalRendererPreparer, SphericalReplacementToken,
    SphericalWorldAreaSummary, SphericalWorldBuildRequest, SphericalWorldBuildTarget,
    SphericalWorldFieldDocument,
};

use field_document::{prepare_control_action, prepare_new_document_display, FieldDocument};
use frame_stats::{emit_runtime_line, FrameSampler};
use natural_display::{LegacyPlanarNaturalFieldDocument, NaturalDisplayError};

use crate::world::spatial::Topology;
use crate::{
    engine::{
        ArtifactError, BuildEngine, BuildFailure, BuildReport, ExternalArtifacts, GraphError,
        MemoryStageCache,
    },
    generators::{
        natural::{
            legacy_planar_natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact,
            GeologicArtifact, GeologicSpecArtifact, HydroErosionArtifact, HydroErosionSpecArtifact,
            MantleArtifact, PreliminaryClimateArtifact, ReliefArtifact,
            ResolvedWorldFormationArtifact, RulePackSetArtifact, TectonicArtifact,
            TectonicRuleResolutionArtifact, TectonicSpecArtifact, WorldFormationSpecArtifact,
        },
        spatial::{PlanarSpaceArtifact, SpatialArtifact},
    },
    gpu::field::CellFieldRenderer,
    resource::{
        CanvasStateResource, FieldDisplayResource, FieldRendererResource, FieldViewerStateResource,
        SphericalPresentationResource,
    },
    rules::{
        default_rule_pack_set, AuthorConstraints, BuiltinRuleError, ConstraintAdoptionOutcome,
        ConstraintSource, RulePackSet, TectonicRuleResolution,
    },
    ui::{
        canvas::canvas::Canvas,
        field::{show_field_controls, show_field_inspector, FieldControlAction},
        spherical::{
            apply_spherical_canvas_action, interact_spherical_canvas, legacy_compatibility_ui,
            queue_spherical_canvas_callback, show_spherical_controls, show_spherical_inspector,
            SphericalCanvasAction, SphericalInspectorCache,
        },
    },
    view::{
        DisplayPrepareError, DisplayRevisionClock, FieldDisplayState, PreparedFieldDisplay,
        VectorGlyphLod,
    },
    world::{
        natural::{
            preliminary_prevailing_wind_m_s_field_id, surface_elevation_m_field_id, ClimateSpec,
            GeologicSpec, GeologicSpecError, HydroErosionSpec, NaturalSpecError, ReliefSpec,
            ResolvedWorldFormation, ResolvedWorldFormationPreset, SeaLevelPolicy, TectonicActivity,
            TectonicSpec, WorldFormationPreset, WorldFormationSpec, WorldFormationSpecError,
            MAX_CONTINENTAL_CRUST_FRACTION, MAX_PLATE_COUNT, MIN_CONTINENTAL_CRUST_FRACTION,
            MIN_PLATE_COUNT,
        },
        BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed, SpecError, TechnologyBaseline,
        WorldSpec, WORLD_SPEC_SCHEMA_V1,
    },
};

const DEFAULT_WORLD_WIDTH_M: f64 = 20_000_000.0;
const DEFAULT_WORLD_HEIGHT_M: f64 = 10_000_000.0;
const DEFAULT_TARGET_CELL_COUNT: u32 = 20_000;
/// Root seed used by a newly authored product world.
pub const PRODUCT_DEFAULT_WORLD_SEED: RootSeed = RootSeed::new(42);
const CURRENT_SLICE_STATUS_TEXT: &str =
    "当前切片：空间 → 板块/地壳 → 地形/地质 → 初步气候 → 水文/侵蚀";
const FORMATION_SLICE_STATUS_TEXT: &str =
    "当前切片：空间 → 演化板块 → 基底/初级地形 → 全球环流 → 耦合地貌（P5）";
const CURRENT_SLICE_SUBTITLE: &str = "前工业·中世纪幻想｜当前时间切片（含水文与地表塑形）";
const INITIAL_PLATE_COUNT_LABEL: &str = "初始板块数";
/// The default quality tier for interactive formation builds.
fn default_formation_quality_profile() -> crate::world::natural::NaturalQualityProfile {
    crate::world::natural::NaturalQualityProfile::Draft
}

/// The authored combo label per quality tier (single source for the selector).
fn quality_tier_label(profile: crate::world::natural::NaturalQualityProfile) -> &'static str {
    match profile {
        crate::world::natural::NaturalQualityProfile::Draft => "草稿 · 约 2 万格 · 1–2 分钟",
        crate::world::natural::NaturalQualityProfile::Standard => "标准 · 约 8 万格 · 3–6 分钟",
        crate::world::natural::NaturalQualityProfile::High => "高 · 约 20 万格 · 实验性离线级",
    }
}

/// The short tier name shown on the published-world status line.
fn quality_tier_short_label(profile: crate::world::natural::NaturalQualityProfile) -> &'static str {
    match profile {
        crate::world::natural::NaturalQualityProfile::Draft => "草稿档",
        crate::world::natural::NaturalQualityProfile::Standard => "标准档",
        crate::world::natural::NaturalQualityProfile::High => "高档（实验性）",
    }
}

/// True when the cached formation profile surface no longer serves a request.
fn formation_surface_key_is_stale(
    cached: Option<(crate::world::natural::NaturalQualityProfile, f64)>,
    profile: crate::world::natural::NaturalQualityProfile,
    radius_m: f64,
) -> bool {
    cached != Some((profile, radius_m))
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FormationAuthoringControlState {
    displayed_land_fraction: f32,
    land_fraction_enabled: bool,
    continental_fraction_enabled: bool,
}

/// Resolves reciprocal control locks from the authored policy and last publication.
fn formation_authoring_control_state(
    pipeline: WorldPipeline,
    relief: &ReliefSpec,
    published: Option<SphericalWorldAreaSummary>,
) -> FormationAuthoringControlState {
    if pipeline == WorldPipeline::LegacyFoundation {
        return FormationAuthoringControlState {
            displayed_land_fraction: relief.target_land_fraction,
            land_fraction_enabled: true,
            continental_fraction_enabled: true,
        };
    }
    match relief.sea_level_policy {
        SeaLevelPolicy::WaterInventory => {
            let displayed_land_fraction = match published {
                Some(SphericalWorldAreaSummary::Formation(summary)) => {
                    summary.actual_land_fraction() as f32
                }
                _ => relief.target_land_fraction,
            };
            FormationAuthoringControlState {
                displayed_land_fraction,
                land_fraction_enabled: false,
                continental_fraction_enabled: true,
            }
        }
        SeaLevelPolicy::TargetLandFraction => FormationAuthoringControlState {
            displayed_land_fraction: relief.target_land_fraction,
            land_fraction_enabled: true,
            continental_fraction_enabled: false,
        },
    }
}

/// Which authoritative generation chain the spherical canvas builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WorldPipeline {
    /// The formation product chain (P2v5→P5); the interactive product default.
    Formation,
    /// The legacy spherical natural-foundation chain.
    ///
    /// Kept for arbitrary-resolution worlds: the formation chain is bound to
    /// the fixed quality-profile resolutions, so the 162-cell worlds used by
    /// unit tests can only run here.
    LegacyFoundation,
}

impl Default for WorldPipeline {
    fn default() -> Self {
        // Unit tests author tiny (162-cell) worlds that only the legacy chain
        // accepts; the interactive product always starts on the formation chain.
        #[cfg(test)]
        {
            Self::LegacyFoundation
        }
        #[cfg(not(test))]
        {
            Self::Formation
        }
    }
}

/// One cached formation profile surface keyed by tier and authored radius.
struct FormationSurfaceCacheEntry {
    profile: crate::world::natural::NaturalQualityProfile,
    radius_m: f64,
    surface: crate::world::spatial::SphericalSurfaceSnapshot,
}

/// A spherical world build running on a worker thread.
struct PendingWorldBuild {
    receiver: std::sync::mpsc::Receiver<WorldBuildCompletion>,
    cancellation: crate::engine::BuildCancellation,
    started_at: std::time::Instant,
    replacement: bool,
}

/// Everything the worker bakes for the amplified display of one world.
struct AmplifiedDisplayBundle {
    mesh: crate::view::AmplifiedSurfaceMesh,
    rivers: Vec<crate::view::RiverPolylineSegment>,
    detail: std::sync::Arc<amplified_mesh::AmplifiedDetailContext>,
    initial_hash: u64,
}

/// One camera snapshot deciding whether the detail selection reruns.
#[derive(Debug, Clone, Copy, PartialEq)]
struct AmplifiedDetailProbe {
    view: crate::view::SphericalPresentationViewState,
    canvas_size: [u32; 2],
}

/// One coalesced camera snapshot for the detail worker.
struct DetailRebuildRequest {
    view: crate::view::SphericalPresentationViewState,
    canvas_size: [f64; 2],
    serial: u64,
    /// The selection hash the UI actually has on the GPU — the single
    /// source of truth the worker compares fresh selections against, so
    /// no worker-side mirror can silently drift from the screen.
    installed_hash: u64,
}

/// One worker answer to one coalesced camera snapshot.
enum DetailRebuildAnswer {
    /// A fresh mesh with its pre-projected map geometry and level-matched
    /// river polylines, for a selection that differed from the echoed
    /// installed hash.
    Refreshed(AmplifiedDetailPayload),
    /// The resolved selection's hash matched the installed hash the
    /// request echoed; the UI checks that premise still holds.
    AlreadyInstalled(u64),
    /// A caught rebuild failure to surface; the worker survives it and
    /// the display keeps the last installed mesh.
    Failed(String),
}

struct DetailRebuildResult {
    answer: DetailRebuildAnswer,
    serial: u64,
}

/// What one drain of the worker answers tells the UI thread to do.
struct DetailPoll {
    /// The newest answer's fresh mesh, when it carried one.
    install: Option<AmplifiedDetailPayload>,
    /// The most recent failure in the batch, to surface on the panel.
    error: Option<String>,
    /// The newest answer skipped against a hash the screen no longer
    /// shows: an install landed after that request echoed its premise,
    /// so the current camera snapshot must be re-requested.
    stale_skip: bool,
}

/// Everything one fresh detail rebuild hands the UI thread. The map
/// projection (including its seam cutting) runs on the worker so the
/// UI thread only rebases and uploads.
struct AmplifiedDetailPayload {
    mesh: crate::view::AmplifiedSurfaceMesh,
    map_vertices: Vec<crate::view::AmplifiedMapVertex>,
    map_indices: Vec<u32>,
    rivers: Vec<crate::view::RiverPolylineSegment>,
    /// The built selection's hash; the UI records it as installed only
    /// after the GPU upload actually ran.
    selection_hash: u64,
}

/// The camera-driven incremental rebuild worker of the amplified display
/// (plan M2 Task 3): camera snapshots coalesce latest-wins, selection and
/// assembly both run off the UI thread against the per-batch cache, and
/// finished meshes swap in whole.
struct AmplifiedDetailEngine {
    request: std::sync::mpsc::Sender<DetailRebuildRequest>,
    results: std::sync::mpsc::Receiver<DetailRebuildResult>,
    sent_serial: u64,
    answered_serial: u64,
    last_probe: Option<AmplifiedDetailProbe>,
    /// The selection hash actually uploaded to the GPU (advanced only
    /// after a successful upload); echoed to the worker in every request.
    installed_hash: u64,
}

impl AmplifiedDetailEngine {
    /// Drains the finished answers. Only the newest one decides what the
    /// screen does: an older payload was built for a camera snapshot the
    /// camera has since left, and installing it over a newer "already
    /// installed" answer parked the display on the wrong mesh (a fast
    /// zoom-and-return froze on the zoomed-in mesh that way, with no
    /// further request until the camera moved again).
    fn drain_answers(&mut self) -> DetailPoll {
        let mut newest: Option<DetailRebuildResult> = None;
        let mut error = None;
        while let Ok(result) = self.results.try_recv() {
            self.answered_serial = self.answered_serial.max(result.serial);
            if let DetailRebuildAnswer::Failed(message) = &result.answer {
                error = Some(message.clone());
            }
            if newest
                .as_ref()
                .is_none_or(|current| result.serial > current.serial)
            {
                newest = Some(result);
            }
        }
        let mut poll = DetailPoll {
            install: None,
            error,
            stale_skip: false,
        };
        match newest.map(|result| result.answer) {
            Some(DetailRebuildAnswer::Refreshed(payload)) => poll.install = Some(payload),
            Some(DetailRebuildAnswer::AlreadyInstalled(hash)) => {
                poll.stale_skip = hash != self.installed_hash;
            }
            Some(DetailRebuildAnswer::Failed(_)) | None => {}
        }
        poll
    }
}

fn spawn_amplified_detail_engine(
    context: std::sync::Arc<amplified_mesh::AmplifiedDetailContext>,
    installed_hash: u64,
) -> AmplifiedDetailEngine {
    let (request, request_receiver) = std::sync::mpsc::channel::<DetailRebuildRequest>();
    let (result_sender, results) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut cache = amplified_mesh::BatchCache::default();
        while let Ok(mut request) = request_receiver.recv() {
            // Latest wins: a burst of camera changes resolves once.
            while let Ok(newer) = request_receiver.try_recv() {
                request = newer;
            }
            // One rebuild failure must never kill the engine: an uncaught
            // panic here used to end the thread silently and freeze the
            // display on the last installed mesh forever (giant stale
            // leaves once the camera zoomed on). Catch it, surface it,
            // and keep serving the next camera snapshot.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let started = std::time::Instant::now();
                let selection = amplified_mesh::select_detail_batches(
                    &context,
                    &request.view,
                    request.canvas_size,
                );
                if selection.hash == request.installed_hash {
                    log::debug!(
                        "detail rebuild #{}: selection {:016x} already installed",
                        request.serial,
                        selection.hash
                    );
                    return DetailRebuildAnswer::AlreadyInstalled(selection.hash);
                }
                let Some(mesh) =
                    amplified_mesh::build_detail_mesh(&context, &selection, &mut cache)
                else {
                    return DetailRebuildAnswer::Failed("细节网格装配失败".to_owned());
                };
                let (map_vertices, map_indices) =
                    crate::view::project_amplified_map(&mesh, request.view.projection());
                log::debug!(
                    "detail rebuild #{}: zoom {:.0}, canvas {:?}, {} leaves, selection {:016x}, {:.0} ms",
                    request.serial,
                    request.view.active_zoom(),
                    request.canvas_size,
                    selection.leaves,
                    selection.hash,
                    started.elapsed().as_secs_f64() * 1e3
                );
                DetailRebuildAnswer::Refreshed(AmplifiedDetailPayload {
                    map_vertices,
                    map_indices,
                    rivers: amplified_mesh::build_river_polylines(&context, &selection),
                    selection_hash: selection.hash,
                    mesh,
                })
            }));
            let answer = outcome.unwrap_or_else(|panic| {
                let message = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("未知 panic");
                DetailRebuildAnswer::Failed(format!("细节重建 panic：{message}"))
            });
            if result_sender
                .send(DetailRebuildResult {
                    answer,
                    serial: request.serial,
                })
                .is_err()
            {
                return;
            }
        }
    });
    AmplifiedDetailEngine {
        request,
        results,
        sent_serial: 0,
        answered_serial: 0,
        last_probe: None,
        installed_hash,
    }
}

#[cfg(test)]
mod amplified_detail_engine_tests {
    use super::{
        AmplifiedDetailEngine, AmplifiedDetailPayload, DetailRebuildAnswer, DetailRebuildResult,
    };

    const INSTALLED: u64 = 0xA;
    const ZOOMED: u64 = 0xB;

    /// A detached engine: the test plays the worker through `answers`.
    fn detached_engine(
        installed_hash: u64,
        sent_serial: u64,
    ) -> (
        AmplifiedDetailEngine,
        std::sync::mpsc::Sender<DetailRebuildResult>,
    ) {
        let (request, _) = std::sync::mpsc::channel();
        let (answers, results) = std::sync::mpsc::channel();
        (
            AmplifiedDetailEngine {
                request,
                results,
                sent_serial,
                answered_serial: 0,
                last_probe: None,
                installed_hash,
            },
            answers,
        )
    }

    fn refreshed(serial: u64, selection_hash: u64) -> DetailRebuildResult {
        let mesh = crate::view::AmplifiedSurfaceMesh::new(
            vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            vec![[0; 4]; 3],
            vec![0, 1, 2],
        )
        .expect("one triangle is a valid amplified mesh");
        DetailRebuildResult {
            answer: DetailRebuildAnswer::Refreshed(AmplifiedDetailPayload {
                mesh,
                map_vertices: Vec::new(),
                map_indices: Vec::new(),
                rivers: Vec::new(),
                selection_hash,
            }),
            serial,
        }
    }

    fn already_installed(serial: u64, hash: u64) -> DetailRebuildResult {
        DetailRebuildResult {
            answer: DetailRebuildAnswer::AlreadyInstalled(hash),
            serial,
        }
    }

    /// Fast zoom-and-return: the zoomed-in payload (#1) and the return
    /// snapshot's "already installed" (#2, echoed before #1 landed)
    /// arrive in one drain. The newest answer wins, so the zoomed-in
    /// mesh is never installed over the camera that already left it.
    #[test]
    fn a_newer_already_installed_answer_drops_an_older_payload() {
        let (mut engine, answers) = detached_engine(INSTALLED, 2);
        answers.send(refreshed(1, ZOOMED)).unwrap();
        answers.send(already_installed(2, INSTALLED)).unwrap();

        let poll = engine.drain_answers();

        assert!(poll.install.is_none());
        assert!(!poll.stale_skip);
        assert!(poll.error.is_none());
        assert_eq!(engine.answered_serial, 2);
        assert_eq!(engine.installed_hash, INSTALLED);
    }

    /// The same race split across frames: #1's payload was installed in
    /// an earlier drain, so #2's skip now names a hash the screen no
    /// longer shows and must trigger a re-request.
    #[test]
    fn an_already_installed_answer_against_a_superseded_hash_is_stale() {
        let (mut engine, answers) = detached_engine(ZOOMED, 2);
        answers.send(already_installed(2, INSTALLED)).unwrap();

        let poll = engine.drain_answers();

        assert!(poll.install.is_none());
        assert!(poll.stale_skip);
        assert_eq!(engine.answered_serial, 2);
    }

    #[test]
    fn the_newest_payload_installs_and_a_failure_in_the_batch_surfaces() {
        let (mut engine, answers) = detached_engine(INSTALLED, 3);
        answers
            .send(DetailRebuildResult {
                answer: DetailRebuildAnswer::Failed("boom".to_owned()),
                serial: 1,
            })
            .unwrap();
        answers.send(refreshed(2, 0xC)).unwrap();
        answers.send(refreshed(3, ZOOMED)).unwrap();

        let poll = engine.drain_answers();

        assert_eq!(
            poll.install.map(|payload| payload.selection_hash),
            Some(ZOOMED)
        );
        assert!(!poll.stale_skip);
        assert_eq!(poll.error.as_deref(), Some("boom"));
        assert_eq!(engine.answered_serial, 3);
    }
}

/// Everything one finished worker build hands back to the UI thread.
struct WorldBuildCompletion {
    result: Result<SphericalPresentationCandidate, String>,
    stage_cache: MemoryStageCache,
    formation_surface: Option<FormationSurfaceCacheEntry>,
    amplified: Option<AmplifiedDisplayBundle>,
}

/// Persisted provenance of the currently authored world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PersistedWorldOrigin {
    /// A pre-spherical application state that must remain on the compatibility graph.
    LegacyPlanarV1,
    /// A world authored by the formal spherical natural graph.
    SphericalV1,
}

fn missing_world_origin_is_legacy() -> PersistedWorldOrigin {
    PersistedWorldOrigin::LegacyPlanarV1
}

/// The graph family selected for runtime initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRuntimeGraph {
    /// The explicitly named planar compatibility graph.
    LegacyPlanarFoundation,
    /// The formal spherical natural foundation graph.
    SphericalNaturalFoundation,
    /// The formal formation-product graph (P2v5→P5).
    SphericalFormation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MigrationFailurePoint {
    None,
    #[cfg(test)]
    GpuPrepare,
}

/// Returns the spherical space specification used by a newly authored product world.
pub fn default_spherical_space_spec() -> crate::world::SphericalSpaceSpec {
    crate::world::SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).expect("the Earth-like default radius is valid"),
        target_cell_count: DEFAULT_TARGET_CELL_COUNT,
    }
}

fn configure_frame_stats_scenario(canvas: &mut crate::ui::spherical::SphericalCanvasState) {
    let mut state = canvas.field_state().clone();
    state.select_fill(surface_elevation_m_field_id());
    state.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
    state.set_vector_lod(VectorGlyphLod::Medium);
    state.select_entity(None);
    canvas.replace_field_state(state);
}

mod spherical_space_spec_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::world::{Meters, SphericalSpaceSpec};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        radius: f64,
        target_cell_count: u32,
    }

    pub fn serialize<S>(spec: &SphericalSpaceSpec, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Wire {
            radius: spec.radius.get(),
            target_cell_count: spec.target_cell_count,
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SphericalSpaceSpec, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Wire::deserialize(deserializer)?;
        let spec = SphericalSpaceSpec {
            radius: Meters::new(wire.radius).map_err(serde::de::Error::custom)?,
            target_cell_count: wire.target_cell_count,
        };
        spec.validate().map_err(serde::de::Error::custom)?;
        Ok(spec)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RuleBuildSummary {
    active_pack_count: usize,
    author_constraint_count: usize,
    satisfied_constraint_count: usize,
    compromised_constraint_count: usize,
}

impl RuleBuildSummary {
    fn from_resolution(resolution: &TectonicRuleResolution) -> Self {
        let mut summary = Self {
            active_pack_count: resolution.resolved_packs().len(),
            ..Self::default()
        };
        for adoption in resolution.adoptions() {
            if matches!(adoption.source(), ConstraintSource::Author(_)) {
                summary.author_constraint_count += 1;
            }
            match adoption.outcome() {
                ConstraintAdoptionOutcome::Satisfied => {
                    summary.satisfied_constraint_count += 1;
                }
                ConstraintAdoptionOutcome::Compromised => {
                    summary.compromised_constraint_count += 1;
                }
            }
        }
        summary
    }
}

/// Persisted UI state plus skipped runtime resources for the current natural slice.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct TemplateApp {
    #[serde(default = "missing_world_origin_is_legacy")]
    world_origin: PersistedWorldOrigin,
    #[serde(
        default = "default_spherical_space_spec",
        with = "spherical_space_spec_serde"
    )]
    spherical_space_spec: crate::world::SphericalSpaceSpec,
    #[serde(default)]
    spherical_canvas_state: crate::ui::spherical::SphericalCanvasState,
    world_seed: u64,
    formation_spec: WorldFormationSpec,
    tectonic_spec: TectonicSpec,
    #[serde(default)]
    relief_spec: ReliefSpec,
    geologic_spec: GeologicSpec,
    #[serde(default)]
    world_pipeline: WorldPipeline,
    #[serde(default = "default_formation_quality_profile")]
    formation_quality_profile: crate::world::natural::NaturalQualityProfile,
    #[serde(skip)]
    formation_surface: Option<FormationSurfaceCacheEntry>,
    #[serde(skip)]
    amplified_mesh: Option<std::sync::Arc<crate::view::AmplifiedSurfaceMesh>>,
    #[serde(skip)]
    amplified_detail: Option<AmplifiedDetailEngine>,
    #[serde(skip)]
    river_polylines: Option<std::sync::Arc<Vec<crate::view::RiverPolylineSegment>>>,
    /// The active mesh's pre-projected map geometry (worker-produced;
    /// recomputed on the UI thread only when the projection changes).
    #[serde(skip)]
    amplified_map_projected:
        Option<std::sync::Arc<(Vec<crate::view::AmplifiedMapVertex>, Vec<u32>)>>,
    #[serde(skip)]
    world_build: Option<PendingWorldBuild>,
    #[serde(skip)]
    canvas_widget: Canvas,
    #[serde(skip)]
    field_renderer: Option<FieldRendererResource>,
    #[serde(skip)]
    render_state: Option<RenderState>,
    #[serde(skip)]
    spherical_presentation: SphericalPresentationResource,
    #[serde(skip)]
    active_runtime_graph: Option<AppRuntimeGraph>,
    #[serde(skip)]
    active_runtime_stage_ids: Vec<String>,
    #[serde(skip)]
    spherical_runtime_error: Option<String>,
    #[serde(skip)]
    spherical_inspector_cache: SphericalInspectorCache,
    #[serde(skip)]
    field_display: FieldDisplayResource,
    #[serde(skip)]
    field_viewer_state: FieldViewerStateResource,
    #[serde(skip)]
    legacy_planar_document: Option<LegacyPlanarNaturalFieldDocument>,
    #[serde(skip)]
    stage_cache: MemoryStageCache,
    #[serde(skip)]
    display_revision_clock: DisplayRevisionClock,
    #[serde(skip)]
    rule_build_summary: RuleBuildSummary,
    #[serde(skip, default = "frame_stats::runtime_frame_sampler")]
    frame_sampler: FrameSampler,
    #[serde(skip)]
    frame_stats_persisted_canvas_state: Option<crate::ui::spherical::SphericalCanvasState>,
}

impl Default for TemplateApp {
    fn default() -> Self {
        let canvas_state = CanvasStateResource::default();
        let field_display = FieldDisplayResource::default();
        let field_viewer_state = FieldViewerStateResource::default();
        Self {
            world_origin: PersistedWorldOrigin::SphericalV1,
            spherical_space_spec: default_spherical_space_spec(),
            spherical_canvas_state: crate::ui::spherical::SphericalCanvasState::default(),
            world_seed: PRODUCT_DEFAULT_WORLD_SEED.raw(),
            formation_spec: WorldFormationSpec::default(),
            tectonic_spec: TectonicSpec::default(),
            relief_spec: ReliefSpec::default(),
            geologic_spec: GeologicSpec::default(),
            world_pipeline: WorldPipeline::default(),
            amplified_mesh: None,
            amplified_map_projected: None,
            amplified_detail: None,
            river_polylines: None,
            formation_quality_profile: default_formation_quality_profile(),
            formation_surface: None,
            world_build: None,
            canvas_widget: Canvas::new(
                canvas_state,
                field_display.clone(),
                field_viewer_state.clone(),
            ),
            field_renderer: None,
            render_state: None,
            spherical_presentation: SphericalPresentationResource::default(),
            active_runtime_graph: None,
            active_runtime_stage_ids: Vec::new(),
            spherical_runtime_error: None,
            spherical_inspector_cache: SphericalInspectorCache::default(),
            field_display,
            field_viewer_state,
            legacy_planar_document: None,
            stage_cache: MemoryStageCache::new(),
            display_revision_clock: DisplayRevisionClock::default(),
            rule_build_summary: RuleBuildSummary::default(),
            frame_sampler: frame_stats::runtime_frame_sampler(),
            frame_stats_persisted_canvas_state: None,
        }
    }
}

impl TemplateApp {
    /// Returns the persisted world provenance used for runtime graph routing.
    pub const fn world_origin(&self) -> PersistedWorldOrigin {
        self.world_origin
    }

    /// Returns the explicit authoring specification for spherical worlds.
    pub const fn spherical_space_spec(&self) -> &crate::world::SphericalSpaceSpec {
        &self.spherical_space_spec
    }

    /// Returns the sole runtime graph family selected by persisted provenance.
    pub const fn runtime_graph(&self) -> AppRuntimeGraph {
        match self.world_origin {
            PersistedWorldOrigin::LegacyPlanarV1 => AppRuntimeGraph::LegacyPlanarFoundation,
            PersistedWorldOrigin::SphericalV1 => AppRuntimeGraph::SphericalNaturalFoundation,
        }
    }

    /// Returns the compatibility notice only for an explicitly legacy world.
    pub const fn legacy_compatibility_notice(&self) -> Option<&'static str> {
        match self.world_origin {
            PersistedWorldOrigin::LegacyPlanarV1 => {
                Some("此状态来自旧平面世界；可用当前作者参数显式重新生成球面世界。")
            }
            PersistedWorldOrigin::SphericalV1 => None,
        }
    }

    /// Returns whether the one-way explicit spherical regeneration action is available.
    pub const fn offers_regenerate_as_spherical(&self) -> bool {
        matches!(self.world_origin, PersistedWorldOrigin::LegacyPlanarV1)
    }

    /// Returns the graph that actually produced the current runtime publication.
    pub const fn active_runtime_graph(&self) -> Option<AppRuntimeGraph> {
        self.active_runtime_graph
    }

    /// Returns stage IDs from the graph that actually produced the current runtime publication.
    pub fn active_runtime_stage_ids(&self) -> Option<&[String]> {
        self.active_runtime_graph
            .is_some()
            .then_some(self.active_runtime_stage_ids.as_slice())
    }

    /// Creates the application, registers the sole active map renderer, and builds the first slice.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::setup_fonts(&cc.egui_ctx);

        let mut app = if let Some(storage) = cc.storage {
            let (mut app, persisted_state_error) = match eframe::Storage::get_string(
                storage,
                eframe::APP_KEY,
            ) {
                None => (Self::default(), None),
                Some(_) => match eframe::get_value(storage, eframe::APP_KEY) {
                    Some(app) => (app, None),
                    None => {
                        let fallback = Self {
                            world_origin: PersistedWorldOrigin::LegacyPlanarV1,
                            ..Self::default()
                        };
                        (
                                fallback,
                                Some(
                                    "persisted application state is invalid; opened in legacy compatibility mode"
                                        .to_owned(),
                                ),
                            )
                    }
                },
            };
            app.field_renderer = None;
            app.render_state = None;
            app.spherical_presentation = SphericalPresentationResource::default();
            app.active_runtime_graph = None;
            app.active_runtime_stage_ids.clear();
            app.spherical_runtime_error = persisted_state_error;
            app
        } else {
            Self::default()
        };

        if app.frame_sampler.is_requested_or_enabled()
            && frame_stats::runtime_medium_wind_scenario_requested()
        {
            app.frame_stats_persisted_canvas_state = Some(app.spherical_canvas_state.clone());
            configure_frame_stats_scenario(&mut app.spherical_canvas_state);
        }

        app.render_state = cc.wgpu_render_state.clone();
        if app.frame_sampler.is_requested_or_enabled() {
            let adapter = cc
                .wgpu_render_state
                .as_ref()
                .map(|render_state| format!("{:?}", render_state.adapter.get_info()))
                .unwrap_or_else(|| "unavailable".to_owned());
            emit_runtime_line(&format!(
                "SEKAI_FRAME_STATS adapter={adapter} window=10s source=egui_input_time scenario={}",
                if app.frame_stats_persisted_canvas_state.is_some() {
                    "elevation+medium_wind"
                } else {
                    "current"
                }
            ));
        }
        match (app.world_origin, cc.wgpu_render_state.as_ref()) {
            (PersistedWorldOrigin::LegacyPlanarV1, render_state) => {
                if let Some(render_state) = render_state {
                    app.field_renderer = Some(app.create_field_renderer_resource(render_state));
                }
                app.generate_legacy_planar_natural_world();
            }
            (PersistedWorldOrigin::SphericalV1, Some(render_state)) => {
                // Unit tests need the world synchronously; the interactive
                // product defers to a worker thread so the window paints
                // immediately with a progress indicator.
                #[cfg(test)]
                if let Err(error) =
                    app.try_start_spherical_world(render_state, MigrationFailurePoint::None)
                {
                    log::error!("spherical natural world build failed: {error}");
                }
                #[cfg(not(test))]
                {
                    let _ = render_state;
                    app.request_spherical_world_build();
                }
            }
            (PersistedWorldOrigin::SphericalV1, None) => {
                log::error!("spherical world requires the wgpu render state");
                app.spherical_runtime_error =
                    Some("spherical world requires the wgpu render state".to_owned());
            }
        }
        app
    }

    /// Explicitly regenerates a legacy world as spherical using the real renderer adapter.
    pub fn try_regenerate_as_spherical(
        &mut self,
        render_state: &RenderState,
    ) -> Result<(), AppRuntimeError> {
        self.validate_legacy_spherical_regeneration()?;
        self.try_initialize_spherical_world(render_state, MigrationFailurePoint::None)
    }

    #[cfg(test)]
    fn try_regenerate_as_spherical_with_failure(
        &mut self,
        render_state: &RenderState,
        failure: MigrationFailurePoint,
    ) -> Result<(), AppRuntimeError> {
        self.validate_legacy_spherical_regeneration()?;
        self.try_initialize_spherical_world(render_state, failure)
    }

    fn validate_legacy_spherical_regeneration(&self) -> Result<(), AppRuntimeError> {
        let publication_present = self.spherical_presentation.read_resource(Option::is_some);
        if self.world_origin != PersistedWorldOrigin::LegacyPlanarV1 || publication_present {
            return Err(AppRuntimeError::InvalidSphericalRegenerationState {
                origin: self.world_origin,
                publication_present,
            });
        }
        Ok(())
    }

    fn try_initialize_spherical_world(
        &mut self,
        render_state: &RenderState,
        failure: MigrationFailurePoint,
    ) -> Result<(), AppRuntimeError> {
        let requested_state = self.spherical_canvas_state.field_state().clone();
        let candidate = match self.world_pipeline {
            WorldPipeline::Formation => {
                let surface = self.formation_profile_surface()?.clone();
                build_spherical_formation_candidate_for_view(
                    RootSeed::new(self.world_seed),
                    self.formation_quality_profile,
                    &surface,
                    &self.formation_spec,
                    &self.tectonic_spec,
                    &self.relief_spec,
                    &self.geologic_spec,
                    &mut self.stage_cache,
                    self.spherical_canvas_state.presentation_view_state(),
                    &requested_state,
                    &DisplayRevisionClock::default(),
                )?
            }
            WorldPipeline::LegacyFoundation => build_spherical_presentation_candidate_for_view(
                RootSeed::new(self.world_seed),
                &self.spherical_space_spec,
                &self.formation_spec,
                &self.tectonic_spec,
                &self.relief_spec,
                &self.geologic_spec,
                &mut self.stage_cache,
                self.spherical_canvas_state.presentation_view_state(),
                &requested_state,
                &DisplayRevisionClock::default(),
            )?,
        };
        self.install_initial_spherical_candidate(candidate, render_state, None, failure)
    }

    /// Publishes one complete standalone candidate through the GPU boundary.
    fn install_initial_spherical_candidate(
        &mut self,
        candidate: SphericalPresentationCandidate,
        render_state: &RenderState,
        amplified: Option<AmplifiedDisplayBundle>,
        failure: MigrationFailurePoint,
    ) -> Result<(), AppRuntimeError> {
        let stage_ids = candidate
            .report()
            .stage_ids()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut renderer = crate::gpu::spherical::SphericalFieldRenderer::new(
            &render_state.device,
            render_state.target_format,
        );
        self.store_amplified_bundle(amplified);
        self.upload_amplified_display(&mut renderer, render_state);
        let published = {
            let mut gpu = SphericalRendererPreparer::new(
                &mut renderer,
                &render_state.device,
                &render_state.queue,
            );
            #[cfg(test)]
            if failure == MigrationFailurePoint::GpuPrepare {
                gpu.fail_next_prepare_for_test();
            }
            #[cfg(not(test))]
            let _ = failure;
            PublishedSphericalPresentation::try_new(candidate, &mut gpu)?
        };

        render_state
            .renderer
            .write()
            .callback_resources
            .insert::<crate::gpu::spherical::SphericalFieldRenderer>(renderer);
        self.spherical_canvas_state
            .replace_field_state(published.state().clone());
        self.spherical_presentation.with_resource(|current| {
            *current = Some(published);
        });
        self.legacy_planar_document = None;
        self.field_renderer = None;
        self.field_display
            .with_resource(|display| *display = Default::default());
        self.field_viewer_state
            .with_resource(|state| *state = Default::default());
        render_state
            .renderer
            .write()
            .callback_resources
            .remove::<FieldRendererResource>();
        self.active_runtime_graph = Some(match self.world_pipeline {
            WorldPipeline::Formation => AppRuntimeGraph::SphericalFormation,
            WorldPipeline::LegacyFoundation => AppRuntimeGraph::SphericalNaturalFoundation,
        });
        self.active_runtime_stage_ids = stage_ids;
        self.world_origin = PersistedWorldOrigin::SphericalV1;
        Ok(())
    }

    /// Stores (or clears) the worker's amplified display bundle and
    /// (re)spawns the camera-driven detail engine for the new world.
    fn store_amplified_bundle(&mut self, amplified: Option<AmplifiedDisplayBundle>) {
        match amplified {
            Some(bundle) => {
                self.amplified_mesh = Some(std::sync::Arc::new(bundle.mesh));
                self.amplified_map_projected = None;
                self.river_polylines = Some(std::sync::Arc::new(bundle.rivers));
                self.amplified_detail = Some(spawn_amplified_detail_engine(
                    bundle.detail,
                    bundle.initial_hash,
                ));
            }
            None => {
                self.amplified_mesh = None;
                self.amplified_map_projected = None;
                self.amplified_detail = None;
                self.river_polylines = None;
            }
        }
    }

    /// Installs finished camera-driven detail meshes without blocking.
    fn poll_amplified_detail(&mut self, ctx: &egui::Context) {
        let Some(engine) = &mut self.amplified_detail else {
            return;
        };
        let poll = engine.drain_answers();
        if poll.stale_skip {
            // Forget the probe so the next frame re-requests the current
            // camera snapshot against the hash the screen really shows.
            engine.last_probe = None;
            ctx.request_repaint();
        }
        let awaiting = engine.sent_serial != engine.answered_serial;
        if let Some(failure) = poll.error {
            self.spherical_runtime_error = Some(failure);
        }
        if let Some(payload) = poll.install {
            let selection_hash = payload.selection_hash;
            self.amplified_mesh = Some(std::sync::Arc::new(payload.mesh));
            self.amplified_map_projected = Some(std::sync::Arc::new((
                payload.map_vertices,
                payload.map_indices,
            )));
            self.river_polylines = Some(std::sync::Arc::new(payload.rivers));
            let mut uploaded = false;
            if let Some(render_state) = self.render_state.clone() {
                let mut egui_renderer = render_state.renderer.write();
                if let Some(renderer) = egui_renderer
                    .callback_resources
                    .get_mut::<crate::gpu::spherical::SphericalFieldRenderer>()
                {
                    self.upload_amplified_display(renderer, &render_state);
                    uploaded = true;
                }
            }
            if let Some(engine) = &mut self.amplified_detail {
                log::debug!(
                    "detail installed {:016x} (answered #{} of #{}, uploaded {})",
                    selection_hash,
                    engine.answered_serial,
                    engine.sent_serial,
                    uploaded
                );
                if uploaded {
                    // The screen now shows this selection.
                    engine.installed_hash = selection_hash;
                } else {
                    // Nothing reached the GPU; forget the probe so the next
                    // frame re-requests and the install retries instead of
                    // freezing on the stale mesh.
                    engine.last_probe = None;
                }
            }
            ctx.request_repaint();
        }
        if awaiting {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    /// Submits the camera snapshot to the detail worker when the camera,
    /// view, projection, or canvas changed (latest wins).
    ///
    /// Gated on the amplified layer actually standing in for the cell
    /// fill, so data-inspection fields never trigger rebuilds.
    fn schedule_amplified_detail(&mut self, rect: egui::Rect) {
        if !self
            .spherical_canvas_state
            .field_state()
            .layer_visibility()
            .amplified
        {
            return;
        }
        let view = self.spherical_canvas_state.presentation_view_state();
        let canvas_size = [f64::from(rect.width()), f64::from(rect.height())];
        if canvas_size
            .into_iter()
            .any(|component| !component.is_finite() || component < 2.0)
        {
            return;
        }
        let Some(engine) = &mut self.amplified_detail else {
            return;
        };
        let probe = AmplifiedDetailProbe {
            view,
            canvas_size: [rect.width().round() as u32, rect.height().round() as u32],
        };
        if engine.last_probe == Some(probe) {
            return;
        }
        engine.last_probe = Some(probe);
        engine.sent_serial += 1;
        if engine
            .request
            .send(DetailRebuildRequest {
                view,
                canvas_size,
                serial: engine.sent_serial,
                installed_hash: engine.installed_hash,
            })
            .is_err()
        {
            // The engine thread is gone; say so instead of freezing the
            // display on a stale mesh without a word.
            self.spherical_runtime_error = Some("细节重建线程已退出".to_owned());
        }
    }

    /// The camera-relative rebase anchors for detail uploads: the map
    /// camera center on the projection plane and the globe front
    /// direction. Any nearby point works — precision degrades smoothly
    /// with distance and every detail swap refreshes the anchors.
    fn amplified_detail_anchors(&self) -> ([f64; 2], [f64; 3]) {
        let view = self.spherical_canvas_state.presentation_view_state();
        let globe_anchor = view.globe_camera().front_direction();
        let map_origin = self
            .amplified_detail
            .as_ref()
            .and_then(|engine| engine.last_probe)
            .and_then(|probe| {
                let canvas_size = [
                    f64::from(probe.canvas_size[0]),
                    f64::from(probe.canvas_size[1]),
                ];
                crate::view::MapScreenTransform::new(
                    view.projection(),
                    view.map_camera(),
                    canvas_size,
                )
                .map(|transform| {
                    let center =
                        transform.to_projection([canvas_size[0] * 0.5, canvas_size[1] * 0.5]);
                    [center.x(), center.y()]
                })
            })
            .filter(|origin| origin.iter().all(|component| component.is_finite()))
            .unwrap_or([0.0; 2]);
        (map_origin, globe_anchor)
    }

    /// Uploads (or clears) the amplified meshes and river polylines.
    fn upload_amplified_display(
        &self,
        renderer: &mut crate::gpu::spherical::SphericalFieldRenderer,
        render_state: &RenderState,
    ) {
        match self.amplified_mesh.as_deref() {
            Some(mesh) => {
                let (map_origin, globe_anchor) = self.amplified_detail_anchors();
                // The worker pre-projects each fresh mesh; only meshes
                // installed without a worker payload (world install,
                // renderer recreation) project here.
                let computed;
                let (vertices, indices): (&[crate::view::AmplifiedMapVertex], &[u32]) =
                    match self.amplified_map_projected.as_deref() {
                        Some((vertices, indices)) => (vertices, indices),
                        None => {
                            let projection = self
                                .spherical_canvas_state
                                .presentation_view_state()
                                .projection();
                            computed = crate::view::project_amplified_map(mesh, projection);
                            (&computed.0, &computed.1)
                        }
                    };
                renderer.set_amplified_map_mesh(
                    &render_state.device,
                    &render_state.queue,
                    vertices,
                    indices,
                    map_origin,
                );
                renderer.set_amplified_globe_mesh(
                    &render_state.device,
                    &render_state.queue,
                    mesh,
                    globe_anchor,
                );
            }
            None => renderer.clear_amplified_meshes(),
        }
        self.upload_river_segments(renderer, render_state);
    }

    /// Uploads (or clears) both presenters' river polyline instances.
    fn upload_river_segments(
        &self,
        renderer: &mut crate::gpu::spherical::SphericalFieldRenderer,
        render_state: &RenderState,
    ) {
        let Some(rivers) = self.river_polylines.as_deref() else {
            renderer.clear_river_segments();
            return;
        };
        let projection = self
            .spherical_canvas_state
            .presentation_view_state()
            .projection();
        let bounds = projection.bounds();
        let half_width = (bounds.max_x() - bounds.min_x()) * 0.5;
        let mut map = Vec::with_capacity(rivers.len());
        let mut globe = Vec::with_capacity(rivers.len());
        for segment in rivers {
            let width_px =
                (1.2 + 0.7 * f32::from(segment.strahler_order.saturating_sub(1))).min(6.0);
            globe.push(crate::gpu::spherical::RiverGlobeSegment {
                start: segment.start,
                end: segment.end,
                width_px,
            });
            let (Some(start), Some(end)) = (
                crate::view::project_unit_direction(projection, segment.start),
                crate::view::project_unit_direction(projection, segment.end),
            ) else {
                continue;
            };
            // Seam-crossing reaches drop from the map: a reach is one cell
            // spacing long, so the gap is a sliver at the outline edge.
            if (start[0] - end[0]).abs() > half_width {
                continue;
            }
            map.push(crate::gpu::spherical::RiverMapSegment {
                start,
                end,
                width_px,
            });
        }
        renderer.set_river_segments(&render_state.device, &render_state.queue, &map, &globe);
    }

    /// Returns the cached formation profile surface, rebuilding it when the
    /// authored radius changed.
    ///
    /// The formation chain is bound to the fixed quality-profile resolutions,
    /// so the authored `target_cell_count` does not apply here; only the
    /// radius participates in the surface identity.
    fn formation_profile_surface(
        &mut self,
    ) -> Result<&crate::world::spatial::SphericalSurfaceSnapshot, AppRuntimeError> {
        let radius_m = self.spherical_space_spec.radius.get();
        let profile = self.formation_quality_profile;
        let stale = formation_surface_key_is_stale(
            self.formation_surface
                .as_ref()
                .map(|entry| (entry.profile, entry.radius_m)),
            profile,
            radius_m,
        );
        if stale {
            let bundle = crate::generators::spatial::ProfileSurfaceBuilder::build(
                profile,
                self.spherical_space_spec.radius,
                &crate::engine::BuildCancellation::new(),
            )?;
            self.formation_surface = Some(FormationSurfaceCacheEntry {
                profile,
                radius_m,
                surface: bundle.authoritative_surface().clone(),
            });
        }
        Ok(&self
            .formation_surface
            .as_ref()
            .expect("the formation surface cache was just filled")
            .surface)
    }

    /// Starts one spherical world build on a worker thread.
    ///
    /// The UI keeps rendering the current publication while the solve runs;
    /// [`Self::poll_world_build`] installs the finished candidate. A second
    /// request while one build is pending is ignored.
    fn request_spherical_world_build(&mut self) {
        if self.world_build.is_some() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let cancellation = crate::engine::BuildCancellation::new();
        let worker_cancellation = cancellation.clone();
        let stage_cache = std::mem::take(&mut self.stage_cache);
        let formation_surface = self.formation_surface.take();
        let pipeline = self.world_pipeline;
        let quality_profile = self.formation_quality_profile;
        let root_seed = RootSeed::new(self.world_seed);
        let space = self.spherical_space_spec.clone();
        let formation_spec = self.formation_spec.clone();
        let tectonic_spec = self.tectonic_spec.clone();
        let relief_spec = self.relief_spec.clone();
        let geologic_spec = self.geologic_spec.clone();
        let view_state = self.spherical_canvas_state.presentation_view_state();
        let requested_state = self.spherical_canvas_state.field_state().clone();
        let replacement = self
            .spherical_presentation
            .read_resource(|current| current.as_ref().map(|p| p.replacement_token()));
        let is_replacement = replacement.is_some();
        std::thread::spawn(move || {
            let mut stage_cache = stage_cache;
            let mut formation_surface = formation_surface;
            let result = (|| -> Result<SphericalPresentationCandidate, String> {
                let target = match pipeline {
                    WorldPipeline::Formation => {
                        let radius_m = space.radius.get();
                        let stale = formation_surface_key_is_stale(
                            formation_surface
                                .as_ref()
                                .map(|entry| (entry.profile, entry.radius_m)),
                            quality_profile,
                            radius_m,
                        );
                        if stale {
                            let bundle = crate::generators::spatial::ProfileSurfaceBuilder::build(
                                quality_profile,
                                space.radius,
                                &worker_cancellation,
                            )
                            .map_err(|error| error.to_string())?;
                            formation_surface = Some(FormationSurfaceCacheEntry {
                                profile: quality_profile,
                                radius_m,
                                surface: bundle.authoritative_surface().clone(),
                            });
                        }
                        crate::app::SphericalWorldBuildTarget::Formation {
                            quality_profile,
                            surface: formation_surface
                                .as_ref()
                                .expect("the worker just filled the surface cache")
                                .surface
                                .clone(),
                        }
                    }
                    WorldPipeline::LegacyFoundation => {
                        crate::app::SphericalWorldBuildTarget::LegacyFoundation { space }
                    }
                };
                run_spherical_world_build(
                    SphericalWorldBuildRequest {
                        root_seed,
                        target,
                        formation: formation_spec,
                        tectonic: tectonic_spec,
                        relief: relief_spec,
                        geologic: geologic_spec,
                        view_state,
                        requested_state,
                        replacement,
                    },
                    &mut stage_cache,
                    &worker_cancellation,
                )
                .map_err(|error| {
                    // Surface the report's error diagnostics: "inspect the
                    // report diagnostics" is useless if nothing displays them.
                    let mut message = error.to_string();
                    if let crate::app::SphericalPresentationError::Build(failure) = &error {
                        for diagnostic in failure.report.diagnostics().iter().filter(|diagnostic| {
                            diagnostic.severity() == crate::engine::DiagnosticSeverity::Error
                        }) {
                            message.push('\n');
                            message.push_str(diagnostic.message());
                        }
                    }
                    message
                })
            })();
            // The amplified subdivision bake rides the same worker: it only
            // exists for formation worlds and never blocks the UI thread.
            // The T1 v2 hierarchical engine is the value source; the bake
            // installs the uniform global selection and the camera-driven
            // detail engine refines it after install (plan M2 Task 3).
            let amplified = if worker_cancellation.is_cancelled() {
                None
            } else {
                result.as_ref().ok().and_then(|candidate| {
                    let document = candidate.document().formation()?;
                    let (sea_level_m, display_radius_m) = document.amplified_color_anchors()?;
                    let evaluator =
                        crate::generators::natural::HierarchicalEvaluator::from_formation_product(
                            document.surface(),
                            document.evolved_compatibility(),
                            document.substrate(),
                            document.formation_snapshot(),
                            root_seed,
                        )
                        .ok()?;
                    let segments = document.formation_snapshot().hydrology().river_segments();
                    let detail = std::sync::Arc::new(amplified_mesh::AmplifiedDetailContext {
                        evaluator,
                        sea_level_m: f64::from(sea_level_m),
                        display_radius_m: f64::from(display_radius_m.max(1.0)),
                        river_cells: segments
                            .iter()
                            .map(|segment| (segment.from().raw(), segment.to().raw()))
                            .collect(),
                        river_orders: segments
                            .iter()
                            .map(|segment| segment.strahler_order())
                            .collect(),
                    });
                    let selection = amplified_mesh::initial_selection(&detail);
                    let mut cache = amplified_mesh::BatchCache::default();
                    let mesh = amplified_mesh::build_detail_mesh(&detail, &selection, &mut cache)?;
                    let rivers = amplified_mesh::build_river_polylines(&detail, &selection);
                    Some(AmplifiedDisplayBundle {
                        mesh,
                        rivers,
                        detail,
                        initial_hash: selection.hash,
                    })
                })
            };
            let _ = sender.send(WorldBuildCompletion {
                result,
                stage_cache,
                formation_surface,
                amplified,
            });
        });
        self.world_build = Some(PendingWorldBuild {
            receiver,
            cancellation,
            started_at: std::time::Instant::now(),
            replacement: is_replacement,
        });
        self.spherical_runtime_error = None;
    }

    /// Installs a finished worker build, or keeps waiting without blocking.
    fn poll_world_build(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.world_build else {
            return;
        };
        match pending.receiver.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.world_build = None;
                self.spherical_runtime_error = Some("世界构建线程意外终止".to_owned());
            }
            Ok(completion) => {
                let was_replacement = pending.replacement;
                let was_cancelled = pending.cancellation.is_cancelled();
                self.world_build = None;
                self.stage_cache = completion.stage_cache;
                if completion.formation_surface.is_some() {
                    self.formation_surface = completion.formation_surface;
                }
                let amplified = completion.amplified;
                match completion.result {
                    Ok(candidate) => {
                        let Some(render_state) = self.render_state.clone() else {
                            self.spherical_runtime_error =
                                Some("渲染状态不可用，无法发布新世界".to_owned());
                            return;
                        };
                        let install = if was_replacement {
                            self.install_replacement_candidate(candidate, &render_state, amplified)
                        } else {
                            self.install_initial_spherical_candidate(
                                candidate,
                                &render_state,
                                amplified,
                                MigrationFailurePoint::None,
                            )
                        };
                        self.spherical_runtime_error = install.err().map(|error| error.to_string());
                    }
                    Err(error) => {
                        self.spherical_runtime_error = if was_cancelled {
                            Some("已取消本次世界构建".to_owned())
                        } else {
                            Some(error)
                        };
                    }
                }
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn try_start_spherical_world(
        &mut self,
        render_state: &RenderState,
        failure: MigrationFailurePoint,
    ) -> Result<(), AppRuntimeError> {
        match self.try_initialize_spherical_world(render_state, failure) {
            Ok(()) => {
                self.spherical_runtime_error = None;
                Ok(())
            }
            Err(error) => {
                self.spherical_runtime_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn try_rebuild_spherical_world(
        &mut self,
        render_state: &RenderState,
    ) -> Result<(), AppRuntimeError> {
        let candidate = match self.world_pipeline {
            WorldPipeline::Formation => {
                let surface = self.formation_profile_surface()?.clone();
                self.spherical_presentation.read_resource(|current| {
                    current
                        .as_ref()
                        .ok_or(AppRuntimeError::MissingSphericalPublication)?
                        .prepare_formation_replacement_candidate_for_view(
                            RootSeed::new(self.world_seed),
                            self.formation_quality_profile,
                            &surface,
                            &self.formation_spec,
                            &self.tectonic_spec,
                            &self.relief_spec,
                            &self.geologic_spec,
                            &mut self.stage_cache,
                            self.spherical_canvas_state.presentation_view_state(),
                            self.spherical_canvas_state.field_state(),
                        )
                        .map_err(AppRuntimeError::from)
                })?
            }
            WorldPipeline::LegacyFoundation => {
                self.spherical_presentation.read_resource(|current| {
                    current
                        .as_ref()
                        .ok_or(AppRuntimeError::MissingSphericalPublication)?
                        .prepare_replacement_candidate_for_view(
                            RootSeed::new(self.world_seed),
                            &self.spherical_space_spec,
                            &self.formation_spec,
                            &self.tectonic_spec,
                            &self.relief_spec,
                            &self.geologic_spec,
                            &mut self.stage_cache,
                            self.spherical_canvas_state.presentation_view_state(),
                            self.spherical_canvas_state.field_state(),
                        )
                        .map_err(AppRuntimeError::from)
                })?
            }
        };
        self.install_replacement_candidate(candidate, render_state, None)
    }

    /// Replaces the whole current publication with one finished candidate.
    fn install_replacement_candidate(
        &mut self,
        candidate: SphericalPresentationCandidate,
        render_state: &RenderState,
        amplified: Option<AmplifiedDisplayBundle>,
    ) -> Result<(), AppRuntimeError> {
        let stage_ids = candidate
            .report()
            .stage_ids()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut egui_renderer = render_state.renderer.write();
        let renderer = egui_renderer
            .callback_resources
            .get_mut::<crate::gpu::spherical::SphericalFieldRenderer>()
            .ok_or(AppRuntimeError::MissingSphericalRenderer)?;
        let mut gpu = SphericalRendererPreparer::new(
            &mut *renderer,
            &render_state.device,
            &render_state.queue,
        );
        let reconciled_state = self.spherical_presentation.with_resource(|current| {
            let current = current
                .as_mut()
                .ok_or(AppRuntimeError::MissingSphericalPublication)?;
            current.try_replace(candidate, &mut gpu)?;
            Ok::<_, AppRuntimeError>(current.state().clone())
        })?;
        self.store_amplified_bundle(amplified);
        self.upload_amplified_display(renderer, render_state);
        drop(egui_renderer);
        self.spherical_canvas_state
            .replace_field_state(reconciled_state);
        self.active_runtime_graph = Some(match self.world_pipeline {
            WorldPipeline::Formation => AppRuntimeGraph::SphericalFormation,
            WorldPipeline::LegacyFoundation => AppRuntimeGraph::SphericalNaturalFoundation,
        });
        self.active_runtime_stage_ids = stage_ids;
        Ok(())
    }

    fn apply_spherical_action(&mut self, action: SphericalCanvasAction) {
        let Some(render_state) = self.render_state.clone() else {
            self.spherical_runtime_error = Some("球面呈现缺少 wgpu render state".to_owned());
            return;
        };
        if matches!(action, SphericalCanvasAction::RegenerateAsSpherical) {
            let result = match self.world_origin {
                PersistedWorldOrigin::LegacyPlanarV1 => {
                    self.try_regenerate_as_spherical(&render_state)
                }
                PersistedWorldOrigin::SphericalV1 => {
                    // Spherical rebuilds run on a worker thread so the UI
                    // keeps rendering the current world during the solve.
                    self.request_spherical_world_build();
                    Ok(())
                }
            };
            self.spherical_runtime_error = result.err().map(|error| error.to_string());
            return;
        }

        let mut egui_renderer = render_state.renderer.write();
        let Some(renderer) = egui_renderer
            .callback_resources
            .get_mut::<crate::gpu::spherical::SphericalFieldRenderer>()
        else {
            self.spherical_runtime_error = Some("球面 callback renderer 尚未注册".to_owned());
            return;
        };
        let result = self.spherical_presentation.with_resource(|current| {
            let current = current
                .as_mut()
                .ok_or_else(|| "球面 publication 尚未建立".to_owned())?;
            apply_spherical_canvas_action(
                current,
                &mut *renderer,
                &render_state.device,
                &render_state.queue,
                &mut self.spherical_canvas_state,
                action,
            )
            .map_err(|error| error.to_string())
        });
        match result {
            Ok(invalidation) => {
                // The amplified map mesh is projection-bound; re-project it
                // whenever the cell map geometry was replaced so both display
                // modes stay valid after a projection or meridian change.
                if invalidation.map_geometry() {
                    if let Some(mesh) = self.amplified_mesh.clone() {
                        let projection = self
                            .spherical_canvas_state
                            .presentation_view_state()
                            .projection();
                        let (map_origin, _) = self.amplified_detail_anchors();
                        let (vertices, indices) =
                            crate::view::project_amplified_map(&mesh, projection);
                        renderer.set_amplified_map_mesh(
                            &render_state.device,
                            &render_state.queue,
                            &vertices,
                            &indices,
                            map_origin,
                        );
                        // Keep the stored projection in step with the
                        // new map geometry for later re-uploads.
                        self.amplified_map_projected =
                            Some(std::sync::Arc::new((vertices, indices)));
                    }
                    self.upload_river_segments(renderer, &render_state);
                }
                self.spherical_runtime_error = None;
            }
            Err(error) => self.spherical_runtime_error = Some(error),
        }
    }

    fn show_active_canvas_after_actions(
        &mut self,
        ctx: &egui::Context,
        actions: Vec<SphericalCanvasAction>,
    ) {
        for action in actions {
            self.apply_spherical_action(action);
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.world_origin {
            PersistedWorldOrigin::LegacyPlanarV1 => {
                ui.add(&mut self.canvas_widget);
            }
            PersistedWorldOrigin::SphericalV1 => {
                let canvas_state = &mut self.spherical_canvas_state;
                let output = self.spherical_presentation.read_resource(|current| {
                    current.as_ref().map(|presentation| {
                        interact_spherical_canvas(ui, presentation, canvas_state)
                    })
                });
                let Some(output) = output else {
                    ui.centered_and_justified(|ui| {
                        ui.label("球面世界尚未发布");
                    });
                    return;
                };
                let rect = output.response().rect;
                for action in output.into_actions() {
                    self.apply_spherical_action(action);
                }
                self.schedule_amplified_detail(rect);
                let queued = self.spherical_presentation.read_resource(|current| {
                    current.as_ref().is_some_and(|presentation| {
                        queue_spherical_canvas_callback(
                            ui,
                            presentation,
                            &self.spherical_canvas_state,
                            rect,
                        );
                        true
                    })
                });
                if !queued {
                    ui.centered_and_justified(|ui| {
                        ui.label("球面世界尚未发布");
                    });
                }
            }
        });
    }

    fn setup_fonts(ctx: &egui::Context) {
        use egui::{FontData, FontDefinitions, FontFamily};

        let mut fonts = FontDefinitions::default();
        let noto_sans_sc = include_bytes!("../assets/fonts/NotoSansSC-Regular.otf");
        fonts.font_data.insert(
            "noto_sans_sc".to_owned(),
            Arc::new(FontData::from_static(noto_sans_sc)),
        );
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "noto_sans_sc".to_owned());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "noto_sans_sc".to_owned());
        ctx.set_fonts(fonts);
    }

    fn create_field_renderer_resource(&self, render_state: &RenderState) -> FieldRendererResource {
        let renderer = CellFieldRenderer::new(&render_state.device, render_state.target_format);
        let resource = FieldRendererResource::new(renderer);
        render_state
            .renderer
            .write()
            .callback_resources
            .insert::<FieldRendererResource>(resource.clone());
        resource
    }

    fn generate_legacy_planar_natural_world(&mut self) {
        let world = default_world_spec(RootSeed::new(self.world_seed));
        let tectonic = self.tectonic_spec.clone();
        if let Err(error) = self.try_replace_legacy_planar_natural_world(&world, &tectonic) {
            log::error!("natural world build failed: {error}");
        }
    }

    fn try_replace_legacy_planar_natural_world(
        &mut self,
        world: &WorldSpec,
        tectonic: &TectonicSpec,
    ) -> Result<(), NaturalWorldBuildError> {
        let current_state = self.field_viewer_state.read_resource(Clone::clone);
        let geologic = self.geologic_spec.clone();
        let candidate = build_legacy_planar_natural_candidate(
            world,
            &self.formation_spec,
            tectonic,
            &geologic,
            &mut self.stage_cache,
            &current_state,
            &self.display_revision_clock,
        );
        self.publish_legacy_planar_natural_candidate(candidate)
    }

    #[cfg(test)]
    fn try_replace_legacy_planar_natural_world_with_rule_inputs(
        &mut self,
        world: &WorldSpec,
        tectonic: &TectonicSpec,
        pack_set: RulePackSet,
        author_constraints: AuthorConstraints,
    ) -> Result<(), NaturalWorldBuildError> {
        let current_state = self.field_viewer_state.read_resource(Clone::clone);
        let geologic = self.geologic_spec.clone();
        let candidate = build_legacy_planar_natural_candidate_with_rule_inputs(
            world,
            &self.formation_spec,
            tectonic,
            &geologic,
            pack_set,
            author_constraints,
            &mut self.stage_cache,
            &current_state,
            &self.display_revision_clock,
        );
        self.publish_legacy_planar_natural_candidate(candidate)
    }

    fn publish_legacy_planar_natural_candidate(
        &mut self,
        candidate: Result<LegacyPlanarNaturalWorldCandidate, NaturalWorldBuildError>,
    ) -> Result<(), NaturalWorldBuildError> {
        let candidate = match candidate {
            Ok(candidate) => candidate,
            Err(error) => {
                self.field_display.with_resource(|resource| {
                    resource
                        .reject_runtime("natural.build", error.to_string())
                        .expect("the built-in natural build status code is valid");
                });
                return Err(error);
            }
        };

        let LegacyPlanarNaturalWorldCandidate {
            document,
            state,
            packet,
            clock,
            report,
            rule_summary,
        } = candidate;
        let stage_ids = report.stage_ids().into_iter().map(str::to_owned).collect();
        for stage in report.stages() {
            log::info!(
                "natural stage {}: {:?}{}",
                stage.stage_id(),
                stage.duration(),
                if stage.cache_hit() { " (cache)" } else { "" }
            );
        }
        let cells = document.spatial.snapshot().cell_count();
        let plates = document.tectonic.snapshot().plates().len();
        let segments = document.tectonic.snapshot().boundary_segments().len();

        self.legacy_planar_document = Some(document);
        self.field_viewer_state
            .with_resource(|current| *current = state);
        self.field_display
            .with_resource(|resource| resource.replace(packet));
        self.display_revision_clock = clock;
        self.rule_build_summary = rule_summary;
        self.active_runtime_graph = Some(AppRuntimeGraph::LegacyPlanarFoundation);
        self.active_runtime_stage_ids = stage_ids;
        log::info!(
            "published natural slice: {cells} cells, {plates} plates, {segments} boundary segments, {} rule packs",
            rule_summary.active_pack_count
        );
        Ok(())
    }

    fn apply_field_control_action(&mut self, action: FieldControlAction) {
        let Some(document) = self.legacy_planar_document.as_ref() else {
            return;
        };
        if let FieldControlAction::InspectField(field) = action {
            let is_registered = document
                .catalog()
                .ok()
                .is_some_and(|catalog| catalog.get(&field).is_some());
            if is_registered {
                self.field_viewer_state
                    .with_resource(|state| state.inspect_field(field));
            }
            return;
        }

        let Some(current) = self
            .field_display
            .read_resource(|resource| resource.current_cloned())
        else {
            return;
        };
        let mut next_state = self.field_viewer_state.read_resource(Clone::clone);
        let mut next_clock = self.display_revision_clock.clone();
        match prepare_control_action(document, &current, &mut next_state, &mut next_clock, action) {
            Ok(packet) => {
                self.field_viewer_state
                    .with_resource(|state| *state = next_state);
                self.field_display
                    .with_resource(|resource| resource.replace(packet));
                self.display_revision_clock = next_clock;
            }
            Err(error) => {
                self.field_display
                    .with_resource(|resource| resource.reject_prepare(error));
            }
        }
    }
}

impl eframe::App for TemplateApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Some(mut persisted) = self.frame_stats_persisted_canvas_state.take() {
            std::mem::swap(&mut self.spherical_canvas_state, &mut persisted);
            eframe::set_value(storage, eframe::APP_KEY, self);
            std::mem::swap(&mut self.spherical_canvas_state, &mut persisted);
            self.frame_stats_persisted_canvas_state = Some(persisted);
        } else {
            eframe::set_value(storage, eframe::APP_KEY, self);
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_world_build(ctx);
        self.poll_amplified_detail(ctx);
        let update_time_seconds = ctx.input(|input| input.time);
        let toggle_frame_sampler = ctx.input(|input| {
            input.modifiers.ctrl && input.modifiers.alt && input.key_pressed(egui::Key::F)
        });
        if toggle_frame_sampler {
            if self.frame_sampler.is_enabled() {
                self.frame_sampler.disable();
            } else {
                self.frame_sampler.start(update_time_seconds);
            }
        }
        self.frame_sampler.observe_update(update_time_seconds);
        if self.frame_sampler.take_viewport_report_request() {
            let viewport_size = ctx.input(|input| {
                input
                    .viewport()
                    .inner_rect
                    .unwrap_or_else(|| input.screen_rect())
                    .size()
            });
            emit_runtime_line(&format!(
                "SEKAI_FRAME_STATS logical_viewport={:.0}x{:.0}",
                viewport_size.x, viewport_size.y
            ));
        }
        if let Some(stats) = self.frame_sampler.take_unreported_completed() {
            emit_runtime_line(&format!(
                "SEKAI_FRAME_STATS average_fps={:.3} one_percent_low_fps={:.3} samples={} window=10s",
                stats.average_fps(),
                stats.one_percent_low_fps(),
                stats.sample_count()
            ));
        }
        if self.frame_sampler.is_sampling() {
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                if !cfg!(target_arch = "wasm32") {
                    ui.menu_button("文件", |ui| {
                        if ui.button("退出").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }
                egui::widgets::global_theme_preference_buttons(ui);
                if self.frame_sampler.is_enabled() {
                    ui.separator();
                    if let Some(stats) = self.frame_sampler.completed() {
                        ui.label(format!(
                            "10s updates: {:.1} FPS | 1% low {:.1} FPS | {} samples",
                            stats.average_fps(),
                            stats.one_percent_low_fps(),
                            stats.sample_count()
                        ));
                    } else {
                        ui.label("10s update sampling…");
                    }
                }
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(match self.active_runtime_graph {
                    Some(AppRuntimeGraph::SphericalFormation) => FORMATION_SLICE_STATUS_TEXT,
                    _ => CURRENT_SLICE_STATUS_TEXT,
                });
                ui.separator();
                ui.hyperlink_to("egui", "https://github.com/emilk/egui");
                egui::warn_if_debug_build(ui);
            });
        });

        let mut field_actions = Vec::new();
        let mut spherical_actions = Vec::new();
        let mut rebuild = false;
        let mut new_seed = false;
        let published_area_summary = self.spherical_presentation.read_resource(|current| {
            current
                .as_ref()
                .map(|presentation| presentation.document().area_summary())
        });
        egui::SidePanel::left("control_panel")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("自然世界");
                    ui.label(CURRENT_SLICE_SUBTITLE);
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label("根种子");
                        ui.add(egui::DragValue::new(&mut self.world_seed).range(0..=u64::MAX));
                    });
                    if ui.button("新种子并重建").clicked() {
                        new_seed = true;
                    }

                    ui.add_space(8.0);
                    let previous_preset = self.formation_spec.preset;
                    egui::ComboBox::from_label("世界形态")
                        .selected_text(formation_preset_label(self.formation_spec.preset))
                        .show_ui(ui, |ui| {
                            for preset in [
                                WorldFormationPreset::Random,
                                WorldFormationPreset::Continents,
                                WorldFormationPreset::Archipelago,
                                WorldFormationPreset::Supercontinent,
                                WorldFormationPreset::GreatIsland,
                                WorldFormationPreset::VolcanicIslands,
                            ] {
                                ui.selectable_value(
                                    &mut self.formation_spec.preset,
                                    preset,
                                    formation_preset_label(preset),
                                );
                            }
                        });
                    if self.formation_spec.preset != previous_preset {
                        let selected = self.formation_spec.preset;
                        apply_formation_preset_selection(
                            &mut self.formation_spec,
                            &mut self.tectonic_spec,
                            &mut self.relief_spec,
                            selected,
                        );
                    }
                    ui.horizontal(|ui| {
                        ui.label(INITIAL_PLATE_COUNT_LABEL);
                        ui.add(
                            egui::DragValue::new(&mut self.tectonic_spec.plate_count)
                                .range(MIN_PLATE_COUNT..=MAX_PLATE_COUNT),
                        );
                    });
                    if self.world_pipeline == WorldPipeline::Formation {
                        ui.horizontal(|ui| {
                            ui.label("驱动");
                            ui.radio_value(
                                &mut self.relief_spec.sea_level_policy,
                                SeaLevelPolicy::WaterInventory,
                                "陆壳比例",
                            )
                            .on_hover_text("物理解：海平面由表层水量与地形共同决定");
                            ui.radio_value(
                                &mut self.relief_spec.sea_level_policy,
                                SeaLevelPolicy::TargetLandFraction,
                                "陆地占比",
                            )
                            .on_hover_text("按目标陆地占比求解海平面，并推算所需海水量");
                        });
                    }
                    let controls = formation_authoring_control_state(
                        self.world_pipeline,
                        &self.relief_spec,
                        published_area_summary,
                    );
                    if controls.land_fraction_enabled {
                        ui.add(
                            egui::Slider::new(
                                &mut self.relief_spec.target_land_fraction,
                                crate::world::natural::MIN_TARGET_LAND_FRACTION
                                    ..=crate::world::natural::MAX_TARGET_LAND_FRACTION,
                            )
                            .text("陆地占比")
                            .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
                        );
                    } else {
                        let mut measured_land_fraction = controls.displayed_land_fraction;
                        ui.add_enabled(
                            false,
                            egui::Slider::new(&mut measured_land_fraction, 0.0..=1.0)
                                .text("陆地占比（上次构建实测）")
                                .custom_formatter(|value, _| format!("{:.1}%", value * 100.0)),
                        )
                        .on_disabled_hover_text("陆壳比例驱动时，陆地占比由物理水线推算");
                    }
                    ui.collapsing("高级", |ui| {
                        ui.add_enabled(
                            controls.continental_fraction_enabled,
                            egui::Slider::new(
                                &mut self.tectonic_spec.continental_crust_fraction,
                                MIN_CONTINENTAL_CRUST_FRACTION..=MAX_CONTINENTAL_CRUST_FRACTION,
                            )
                            .text(if controls.continental_fraction_enabled {
                                "初始大陆地壳比例"
                            } else {
                                "初始大陆地壳比例（预设值）"
                            })
                            .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
                        )
                        .on_disabled_hover_text("陆地占比驱动时，陆壳比例锁定为当前作者值");
                    });
                    egui::ComboBox::from_label("构造活动")
                        .selected_text(activity_label(self.tectonic_spec.activity))
                        .show_ui(ui, |ui| {
                            for activity in [
                                TectonicActivity::Quiet,
                                TectonicActivity::Moderate,
                                TectonicActivity::Active,
                            ] {
                                ui.selectable_value(
                                    &mut self.tectonic_spec.activity,
                                    activity,
                                    activity_label(activity),
                                );
                            }
                        });
                    if self.world_pipeline == WorldPipeline::Formation {
                        egui::ComboBox::from_label("质量档位")
                            .selected_text(quality_tier_label(self.formation_quality_profile))
                            .show_ui(ui, |ui| {
                                for profile in [
                                    crate::world::natural::NaturalQualityProfile::Draft,
                                    crate::world::natural::NaturalQualityProfile::Standard,
                                    crate::world::natural::NaturalQualityProfile::High,
                                ] {
                                    ui.selectable_value(
                                        &mut self.formation_quality_profile,
                                        profile,
                                        quality_tier_label(profile),
                                    );
                                }
                            });
                    }
                    if ui.button("按当前参数重建").clicked() {
                        rebuild = true;
                    }
                    if let Some(pending) = &self.world_build {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new());
                            ui.label(format!(
                                "正在生成世界…已用 {:.0} 秒",
                                pending.started_at.elapsed().as_secs_f32()
                            ));
                            if ui.button("取消").clicked() {
                                pending.cancellation.cancel();
                            }
                        });
                    }

                    if let Some(compatibility) = legacy_compatibility_ui(self) {
                        ui.separator();
                        ui.label(compatibility.notice());
                        if ui.button(compatibility.action_label()).clicked() {
                            spherical_actions.push(SphericalCanvasAction::RegenerateAsSpherical);
                        }
                    }

                    if let Some(document) = self.legacy_planar_document.as_ref() {
                        ui.separator();
                        ui.label(format!(
                            "{} 个单元｜{} 个板块｜{} 条边界段",
                            document.spatial.snapshot().cell_count(),
                            document.tectonic.snapshot().plates().len(),
                            document.tectonic.snapshot().boundary_segments().len()
                        ));
                        ui.label(formation_provenance_label(document.formation.formation()));
                        ui.label(format!(
                            "规则包 {}｜作者约束 {}｜满足 {}｜妥协 {}",
                            self.rule_build_summary.active_pack_count,
                            self.rule_build_summary.author_constraint_count,
                            self.rule_build_summary.satisfied_constraint_count,
                            self.rule_build_summary.compromised_constraint_count,
                        ));
                        let catalog = document
                            .catalog()
                            .expect("the stored natural display document is validated");
                        let state = self.field_viewer_state.read_resource(Clone::clone);
                        field_actions.extend(show_field_controls(ui, &catalog, &state));
                        ui.separator();
                        let diagnostics: Vec<_> = document
                            .diagnostics()
                            .iter()
                            .map(|diagnostic| diagnostic.as_ref())
                            .collect();
                        show_field_inspector(ui, &catalog, &state, &diagnostics);
                    } else {
                        self.spherical_presentation.read_resource(|current| {
                            let Some(presentation) = current.as_ref() else {
                                return;
                            };
                            ui.separator();
                            ui.label(match presentation.document().quality_profile() {
                                Some(profile) => format!(
                                    "{} 个球面单元｜{}｜单位球呈现",
                                    presentation.globe().cell_count(),
                                    quality_tier_short_label(profile),
                                ),
                                None => format!(
                                    "{} 个球面单元｜单位球呈现",
                                    presentation.globe().cell_count()
                                ),
                            });
                            show_spherical_area_summary(ui, presentation.document().area_summary());
                            match show_spherical_controls(
                                ui,
                                presentation,
                                &self.spherical_canvas_state,
                            ) {
                                Ok(actions) => spherical_actions.extend(actions),
                                Err(error) => {
                                    ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
                                }
                            }
                            match self.spherical_inspector_cache.model(
                                presentation,
                                self.spherical_canvas_state.field_state(),
                                self.spherical_canvas_state.view_mode(),
                            ) {
                                Ok(model) => show_spherical_inspector(ui, model),
                                Err(error) => {
                                    ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
                                }
                            }
                        });
                    }

                    if let Some(status) = self
                        .field_display
                        .read_resource(|display| display.error().map(ToString::to_string))
                    {
                        ui.separator();
                        ui.colored_label(egui::Color32::LIGHT_RED, status);
                    }
                    if let Some(status) = self.spherical_runtime_error.as_deref() {
                        ui.separator();
                        ui.colored_label(egui::Color32::LIGHT_RED, status);
                    }
                    // A pending detail rebuild is visible state, not a
                    // mystery: under heavy load (a world rebuild solving on
                    // this process's cores, or another program saturating
                    // the machine) a rebuild can take seconds, and the
                    // stale mesh on screen looks like giant primitives at
                    // deep zoom until the swap lands.
                    if self
                        .amplified_detail
                        .as_ref()
                        .is_some_and(|engine| engine.sent_serial != engine.answered_serial)
                    {
                        ui.separator();
                        ui.weak("细节重建中…");
                    }
                });
            });

        for action in field_actions {
            self.apply_field_control_action(action);
        }
        if new_seed {
            self.world_seed = rand::random();
            rebuild = true;
        }
        if rebuild {
            match self.world_origin {
                PersistedWorldOrigin::LegacyPlanarV1 => {
                    self.generate_legacy_planar_natural_world();
                }
                PersistedWorldOrigin::SphericalV1 => {
                    spherical_actions.push(SphericalCanvasAction::RegenerateAsSpherical);
                }
            }
        }

        self.show_active_canvas_after_actions(ctx, std::mem::take(&mut spherical_actions));
    }
}

fn activity_label(activity: TectonicActivity) -> &'static str {
    match activity {
        TectonicActivity::Quiet => "宁静",
        TectonicActivity::Moderate => "适中",
        TectonicActivity::Active => "活跃",
    }
}

fn show_spherical_area_summary(ui: &mut egui::Ui, summary: SphericalWorldAreaSummary) {
    match summary {
        SphericalWorldAreaSummary::NaturalFoundation(summary) => {
            show_natural_area_summary(ui, summary)
        }
        SphericalWorldAreaSummary::Formation(summary) => show_formation_area_summary(ui, summary),
    }
}

fn show_natural_area_summary(ui: &mut egui::Ui, summary: SphericalNaturalAreaSummary) {
    ui.group(|ui| {
        ui.strong("面积依从性");
        ui.label(format!(
            "初始大陆地壳面积：作者 {:.1}%｜演化后 {:.1}%",
            summary.requested_initial_continental_crust_fraction() * 100.0,
            summary.evolved_continental_crust_fraction() * 100.0,
        ));
        ui.label(format!(
            "陆地面积：目标 {:.1}%｜实际 {:.1}%｜偏差 {:+.1} 个百分点",
            summary.target_land_fraction() * 100.0,
            summary.actual_land_fraction() * 100.0,
            (summary.actual_land_fraction() - summary.target_land_fraction()) * 100.0,
        ));
        ui.label(format!("海平面：{:.1} m", summary.sea_level_m()));
    });
}

fn show_formation_area_summary(ui: &mut egui::Ui, summary: crate::app::FormationAreaSummary) {
    ui.group(|ui| {
        ui.strong("面积依从性（P5 形成链）");
        ui.label(format!(
            "大陆地壳面积：作者 {:.1}%｜演化后 {:.1}%（材料守恒）",
            summary.authored_continental_fraction() * 100.0,
            summary.evolved_continental_fraction() * 100.0,
        ));
        ui.label(format!(
            "陆地面积：目标 {:.1}%｜实际 {:.1}%｜偏差 {:+.1} 个百分点",
            summary.target_land_fraction() * 100.0,
            summary.actual_land_fraction() * 100.0,
            (summary.actual_land_fraction() - f64::from(summary.target_land_fraction())) * 100.0,
        ));
        ui.label(format!(
            "海水量 = {:.3} × 地球",
            summary.water_inventory_ratio()
        ));
        if !(crate::world::natural::WATER_INVENTORY_RATIO_ADVISORY_MIN
            ..=crate::world::natural::WATER_INVENTORY_RATIO_ADVISORY_MAX)
            .contains(&summary.water_inventory_ratio())
        {
            ui.label(format!(
                "提示：海水量超出建议带 {:.1}–{:.1} × 地球；数值保留，不会钳制",
                crate::world::natural::WATER_INVENTORY_RATIO_ADVISORY_MIN,
                crate::world::natural::WATER_INVENTORY_RATIO_ADVISORY_MAX,
            ));
        }
        if summary.sea_level_policy() == SeaLevelPolicy::TargetLandFraction
            && f64::from(summary.target_land_fraction())
                > crate::world::natural::OCEAN_FLOOR_EXPOSURE_HINT_FRACTION
                    * summary.evolved_continental_fraction()
        {
            ui.label("提示：该目标将露出洋底；过程仍按物理水线求解");
        }
        ui.label(format!("海平面：{:.1} m", summary.sea_level_m()));
    });
}

fn formation_preset_label(preset: WorldFormationPreset) -> &'static str {
    match preset {
        WorldFormationPreset::Random => "随机（按种子）",
        WorldFormationPreset::Continents => "多大陆",
        WorldFormationPreset::Archipelago => "群岛",
        WorldFormationPreset::Supercontinent => "超级大陆",
        WorldFormationPreset::GreatIsland => "大岛与卫星岛",
        WorldFormationPreset::VolcanicIslands => "火山群岛",
    }
}

fn resolved_formation_preset_label(preset: ResolvedWorldFormationPreset) -> &'static str {
    match preset {
        ResolvedWorldFormationPreset::Continents => "多大陆",
        ResolvedWorldFormationPreset::Archipelago => "群岛",
        ResolvedWorldFormationPreset::Supercontinent => "超级大陆",
        ResolvedWorldFormationPreset::GreatIsland => "大岛与卫星岛",
        ResolvedWorldFormationPreset::VolcanicIslands => "火山群岛",
    }
}

fn formation_provenance_label(formation: &ResolvedWorldFormation) -> String {
    if formation.requested() == WorldFormationPreset::Random {
        format!(
            "世界形态：{} → {}",
            formation_preset_label(formation.requested()),
            resolved_formation_preset_label(formation.resolved())
        )
    } else {
        format!(
            "世界形态：{}",
            formation_preset_label(formation.requested())
        )
    }
}

fn apply_formation_preset_selection(
    formation: &mut WorldFormationSpec,
    tectonic: &mut TectonicSpec,
    relief: &mut ReliefSpec,
    selected: WorldFormationPreset,
) {
    formation.preset = selected;
    let resolved = match selected {
        WorldFormationPreset::Random => None,
        WorldFormationPreset::Continents => Some(ResolvedWorldFormationPreset::Continents),
        WorldFormationPreset::Archipelago => Some(ResolvedWorldFormationPreset::Archipelago),
        WorldFormationPreset::Supercontinent => Some(ResolvedWorldFormationPreset::Supercontinent),
        WorldFormationPreset::GreatIsland => Some(ResolvedWorldFormationPreset::GreatIsland),
        WorldFormationPreset::VolcanicIslands => {
            Some(ResolvedWorldFormationPreset::VolcanicIslands)
        }
    };
    if let Some(resolved) = resolved {
        tectonic.continental_crust_fraction = resolved.recommended_continental_crust_fraction();
        relief.target_land_fraction = resolved.recommended_land_fraction();
    }
}

fn default_world_spec(root_seed: RootSeed) -> WorldSpec {
    WorldSpec {
        schema_version: WORLD_SPEC_SCHEMA_V1,
        root_seed,
        space: PlanarSpaceSpec {
            width: Meters::new(DEFAULT_WORLD_WIDTH_M)
                .expect("the built-in world width is finite and positive"),
            height: Meters::new(DEFAULT_WORLD_HEIGHT_M)
                .expect("the built-in world height is finite and positive"),
            target_cell_count: DEFAULT_TARGET_CELL_COUNT,
            boundary: BoundaryCondition::Closed,
        },
        technology: TechnologyBaseline::PreIndustrialMedieval,
    }
}

fn build_legacy_planar_natural_external_artifacts(
    world: &WorldSpec,
    formation: &WorldFormationSpec,
    tectonic: &TectonicSpec,
    geologic: &GeologicSpec,
) -> Result<ExternalArtifacts, NaturalWorldBuildError> {
    build_legacy_planar_natural_external_artifacts_with_rule_inputs(
        world,
        formation,
        tectonic,
        geologic,
        default_rule_pack_set()?,
        AuthorConstraints::default(),
    )
}

fn build_legacy_planar_natural_external_artifacts_with_rule_inputs(
    world: &WorldSpec,
    formation: &WorldFormationSpec,
    tectonic: &TectonicSpec,
    geologic: &GeologicSpec,
    pack_set: RulePackSet,
    author_constraints: AuthorConstraints,
) -> Result<ExternalArtifacts, NaturalWorldBuildError> {
    world.validate()?;
    formation.validate()?;
    tectonic.validate()?;
    geologic.validate()?;
    let mut external = ExternalArtifacts::new();
    external.insert(PlanarSpaceArtifact::new(world.space.clone()))?;
    external.insert(TectonicSpecArtifact::new(tectonic.clone()))?;
    external.insert(GeologicSpecArtifact::new(geologic.clone()))?;
    external.insert(ClimateSpecArtifact::new(ClimateSpec::default()))?;
    external.insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))?;
    external.insert(WorldFormationSpecArtifact::new(formation.clone()))?;
    external.insert(RulePackSetArtifact::new(pack_set))?;
    external.insert(AuthorConstraintsArtifact::new(author_constraints))?;
    Ok(external)
}

fn build_legacy_planar_natural_candidate(
    world: &WorldSpec,
    formation: &WorldFormationSpec,
    tectonic: &TectonicSpec,
    geologic: &GeologicSpec,
    cache: &mut MemoryStageCache,
    current_state: &FieldDisplayState,
    clock: &DisplayRevisionClock,
) -> Result<LegacyPlanarNaturalWorldCandidate, NaturalWorldBuildError> {
    let external =
        build_legacy_planar_natural_external_artifacts(world, formation, tectonic, geologic)?;
    build_legacy_planar_natural_candidate_from_external(
        world.root_seed,
        external,
        cache,
        current_state,
        clock,
    )
}

#[cfg(test)]
fn build_legacy_planar_natural_candidate_with_rule_inputs(
    world: &WorldSpec,
    formation: &WorldFormationSpec,
    tectonic: &TectonicSpec,
    geologic: &GeologicSpec,
    pack_set: RulePackSet,
    author_constraints: AuthorConstraints,
    cache: &mut MemoryStageCache,
    current_state: &FieldDisplayState,
    clock: &DisplayRevisionClock,
) -> Result<LegacyPlanarNaturalWorldCandidate, NaturalWorldBuildError> {
    let external = build_legacy_planar_natural_external_artifacts_with_rule_inputs(
        world,
        formation,
        tectonic,
        geologic,
        pack_set,
        author_constraints,
    )?;
    build_legacy_planar_natural_candidate_from_external(
        world.root_seed,
        external,
        cache,
        current_state,
        clock,
    )
}

fn build_legacy_planar_natural_candidate_from_external(
    root_seed: RootSeed,
    external: ExternalArtifacts,
    cache: &mut MemoryStageCache,
    current_state: &FieldDisplayState,
    clock: &DisplayRevisionClock,
) -> Result<LegacyPlanarNaturalWorldCandidate, NaturalWorldBuildError> {
    let outcome = BuildEngine::new(legacy_planar_natural_foundation_graph()?)
        .build(root_seed, external, cache)?;
    let rule_resolution = outcome.artifacts.get::<TectonicRuleResolutionArtifact>()?;
    let rule_summary = RuleBuildSummary::from_resolution(rule_resolution.resolution());
    let spatial = outcome.artifacts.get::<SpatialArtifact>()?;
    let formation = outcome.artifacts.get::<ResolvedWorldFormationArtifact>()?;
    let tectonic = outcome.artifacts.get::<TectonicArtifact>()?;
    let mantle = outcome.artifacts.get::<MantleArtifact>()?;
    let relief = outcome.artifacts.get::<ReliefArtifact>()?;
    let geology = outcome.artifacts.get::<GeologicArtifact>()?;
    let climate = outcome.artifacts.get::<PreliminaryClimateArtifact>()?;
    let hydro_erosion = outcome.artifacts.get::<HydroErosionArtifact>()?;
    let document = LegacyPlanarNaturalFieldDocument::build(
        spatial,
        formation,
        tectonic,
        mantle,
        relief,
        geology,
        climate,
        hydro_erosion,
        &outcome.report,
    )?;
    let mut next_clock = clock.clone();
    let (state, packet) = prepare_new_document_display(&document, current_state, &mut next_clock)?;
    Ok(LegacyPlanarNaturalWorldCandidate {
        document,
        state,
        packet,
        clock: next_clock,
        report: outcome.report,
        rule_summary,
    })
}

struct LegacyPlanarNaturalWorldCandidate {
    document: LegacyPlanarNaturalFieldDocument,
    state: FieldDisplayState,
    packet: Arc<PreparedFieldDisplay>,
    clock: DisplayRevisionClock,
    report: BuildReport,
    rule_summary: RuleBuildSummary,
}

/// Runtime initialization or explicit spherical-regeneration failures.
#[derive(Debug, Error)]
pub enum AppRuntimeError {
    /// The formal spherical candidate or GPU publication failed atomically.
    #[error(transparent)]
    SphericalPresentation(#[from] SphericalPresentationError),
    /// The formation profile surface could not be constructed for the radius.
    #[error(transparent)]
    FormationSurface(#[from] crate::generators::spatial::ProfileSurfaceBuildError),
    /// Spherical runtime state was requested before a publication existed.
    #[error("spherical runtime has no current publication")]
    MissingSphericalPublication,
    /// The spherical callback renderer was not registered.
    #[error("spherical callback renderer is not registered")]
    MissingSphericalRenderer,
    /// The one-way legacy migration was requested outside its sole valid runtime state.
    #[error(
        "spherical regeneration requires LegacyPlanarV1 with no spherical publication (origin: {origin:?}, publication present: {publication_present})"
    )]
    InvalidSphericalRegenerationState {
        /// Persisted origin observed before any build or GPU work.
        origin: PersistedWorldOrigin,
        /// Whether an authoritative spherical publication already exists.
        publication_present: bool,
    },
}

#[derive(Debug, Error)]
enum NaturalWorldBuildError {
    #[error(transparent)]
    WorldSpec(#[from] SpecError),
    #[error(transparent)]
    TectonicSpec(#[from] NaturalSpecError),
    #[error(transparent)]
    WorldFormationSpec(#[from] WorldFormationSpecError),
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
    NaturalDisplay(#[from] NaturalDisplayError),
    #[error(transparent)]
    Display(#[from] DisplayPrepareError),
}

#[cfg(test)]
mod natural_app_tests {
    use std::sync::Arc;

    use super::{
        apply_formation_preset_selection, build_legacy_planar_natural_external_artifacts,
        configure_frame_stats_scenario, default_world_spec, formation_authoring_control_state,
        formation_provenance_label, show_formation_area_summary, show_spherical_area_summary,
        AppRuntimeError, AppRuntimeGraph, FormationAreaSummary, MigrationFailurePoint,
        NaturalWorldBuildError, PersistedWorldOrigin, PublishedSphericalPresentation,
        SphericalWorldAreaSummary, TemplateApp, WorldPipeline, CURRENT_SLICE_STATUS_TEXT,
        CURRENT_SLICE_SUBTITLE, DEFAULT_TARGET_CELL_COUNT, INITIAL_PLATE_COUNT_LABEL,
    };
    use crate::engine::ExternalArtifacts;
    use crate::generators::natural::{
        AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicSpecArtifact,
        HydroErosionSpecArtifact, RulePackSetArtifact, TectonicSpecArtifact,
        WorldFormationSpecArtifact,
    };
    use crate::generators::spatial::PlanarSpaceArtifact;
    use crate::rules::{
        default_rule_pack_set, earthlike_rule_pack, AuthorConstraint, AuthorConstraints,
        CapabilityContribution, ConstraintStrength, CoreSchemaRange, RuleItemId, RulePack,
        RulePackId, RulePackKind, RulePackSet, RuleTectonicConstraint, RuleVersion,
        TectonicConstraintClause, AUTHOR_CONSTRAINTS_SCHEMA_V1,
    };
    use crate::ui::spherical::{
        SphericalCanvasAction, SphericalCanvasState, SphericalInspectorCache,
    };
    use crate::view::{
        FieldDisplayResourceState, OwnedViewDiagnostic, PreparedSphericalOverlay,
        SphericalProjectionKind, SphericalViewMode, VectorGlyphLod, ViewDiagnosticSeverity,
    };
    use crate::world::fields::FieldId;
    use crate::world::natural::{
        boundary_strength_field_id, land_ocean_field_id,
        preliminary_mean_air_temperature_c_field_id, preliminary_prevailing_wind_m_s_field_id,
        surface_elevation_m_field_id, ClimateSpec, GeologicSpec, HydroErosionSpec, MantleActivity,
        ReliefSpec, ResolvedWorldFormationPreset, SeaLevelPolicy, TectonicActivity, TectonicSpec,
        WorldFormationPreset, WorldFormationSpec,
    };
    use crate::world::spatial::Topology;
    use crate::world::{AuthorObjectId, RootSeed, TechnologyBaseline};

    fn request_test_render_state() -> eframe::egui_wgpu::RenderState {
        use eframe::egui_wgpu::{self, wgpu};
        use std::sync::Arc;

        pollster::block_on(async {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: true,
                    compatible_surface: None,
                })
                .await
                .or_else(|| {
                    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::LowPower,
                        force_fallback_adapter: false,
                        compatible_surface: None,
                    }))
                })
                .expect("Task 10 app tests require a compatible GPU adapter");
            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("Task 10 App Test Device"),
                        required_limits: wgpu::Limits::downlevel_defaults(),
                        ..Default::default()
                    },
                    None,
                )
                .await
                .expect("Task 10 app tests require a compatible GPU device");
            let format = wgpu::TextureFormat::Rgba8UnormSrgb;
            let renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);
            egui_wgpu::RenderState {
                adapter,
                available_adapters: Vec::new(),
                device,
                queue,
                target_format: format,
                renderer: Arc::new(egui::mutex::RwLock::new(renderer)),
            }
        })
    }

    #[test]
    fn spherical_authoring_names_the_parameter_as_an_initial_plate_count() {
        assert_eq!(INITIAL_PLATE_COUNT_LABEL, "初始板块数");
    }

    #[derive(Default)]
    struct TestStorage(std::collections::BTreeMap<String, String>);

    impl eframe::Storage for TestStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }

        fn flush(&mut self) {}
    }

    #[test]
    fn opt_in_frame_scenario_is_elevation_medium_wind_and_never_persists() {
        let mut original = SphericalCanvasState::default();
        original
            .apply(SphericalCanvasAction::SelectOverlay(Some(
                boundary_strength_field_id(),
            )))
            .unwrap();
        let mut measured = original.clone();
        configure_frame_stats_scenario(&mut measured);
        assert_eq!(
            measured.field_state().fill_field(),
            Some(&surface_elevation_m_field_id())
        );
        assert_eq!(
            measured.field_state().overlay_field(),
            Some(&preliminary_prevailing_wind_m_s_field_id())
        );
        assert_eq!(measured.field_state().vector_lod(), VectorGlyphLod::Medium);

        let mut app = TemplateApp {
            spherical_canvas_state: measured.clone(),
            frame_stats_persisted_canvas_state: Some(original.clone()),
            ..TemplateApp::default()
        };
        let mut storage = TestStorage::default();
        eframe::App::save(&mut app, &mut storage);

        let saved: TemplateApp = eframe::get_value(&storage, eframe::APP_KEY).unwrap();
        assert_eq!(saved.spherical_canvas_state, original);
        assert_eq!(app.spherical_canvas_state, measured);
    }

    fn create_from_persisted(
        mut persisted: TemplateApp,
        render_state: &eframe::egui_wgpu::RenderState,
    ) -> TemplateApp {
        persisted.spherical_space_spec.target_cell_count = 162;
        let mut storage = TestStorage::default();
        eframe::set_value(&mut storage, eframe::APP_KEY, &persisted);
        let restored: TemplateApp =
            eframe::get_value(&storage, eframe::APP_KEY).unwrap_or_else(|| {
                panic!(
                "the Task 10 persisted app fixture must round-trip through eframe storage: {:?}",
                storage.0.get(eframe::APP_KEY)
            )
            });
        assert_eq!(restored.world_origin, persisted.world_origin);
        let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
        cc.storage = Some(&storage);
        cc.wgpu_render_state = Some(render_state.clone());
        TemplateApp::new(&cc)
    }

    #[test]
    fn template_app_new_executes_only_the_persisted_origin_graph() {
        let render_state = request_test_render_state();

        let legacy = TemplateApp {
            world_origin: PersistedWorldOrigin::LegacyPlanarV1,
            ..TemplateApp::default()
        };
        let legacy = create_from_persisted(legacy, &render_state);
        assert_eq!(
            legacy.active_runtime_graph(),
            Some(AppRuntimeGraph::LegacyPlanarFoundation)
        );
        assert!(legacy.legacy_planar_document.is_some());
        assert!(legacy.spherical_presentation.read_resource(Option::is_none));
        assert!(legacy
            .active_runtime_stage_ids()
            .unwrap()
            .iter()
            .any(|stage| stage == "spatial.planar-voronoi"));
        assert!(legacy
            .active_runtime_stage_ids()
            .unwrap()
            .iter()
            .all(|stage| !stage.starts_with("natural.spherical-")));

        let spherical = create_from_persisted(TemplateApp::default(), &render_state);
        assert_eq!(
            spherical.active_runtime_graph(),
            Some(AppRuntimeGraph::SphericalNaturalFoundation)
        );
        assert!(spherical.legacy_planar_document.is_none());
        assert!(spherical
            .spherical_presentation
            .read_resource(Option::is_some));
        assert!(spherical
            .active_runtime_stage_ids()
            .unwrap()
            .iter()
            .any(|stage| stage == "spatial.spherical-voronoi"));
        assert!(spherical
            .active_runtime_stage_ids()
            .unwrap()
            .iter()
            .all(|stage| *stage != "spatial.planar-voronoi"));
        assert!(render_state
            .renderer
            .read()
            .callback_resources
            .get::<crate::gpu::spherical::SphericalFieldRenderer>()
            .is_some());
    }

    #[test]
    fn persisted_edges_without_a_valid_edge_overlay_reconcile_before_first_inspector_frame() {
        let render_state = request_test_render_state();
        let invalid_overlay = FieldId::new("test.spherical", "missing-overlay", 1).unwrap();
        for (case, overlay) in [
            ("none", None),
            ("vector", Some(preliminary_prevailing_wind_m_s_field_id())),
            ("invalid", Some(invalid_overlay.clone())),
        ] {
            let mut persisted = TemplateApp::default();
            persisted
                .spherical_canvas_state
                .apply(SphericalCanvasAction::SelectOverlay(overlay))
                .unwrap();
            persisted
                .spherical_canvas_state
                .apply(SphericalCanvasAction::SelectEntity(Some(
                    crate::view::SelectedSurfaceEntity::Edge(crate::world::EdgeId::from_raw(0)),
                )))
                .unwrap();
            let mut app = create_from_persisted(persisted, &render_state);
            assert_eq!(
                app.spherical_canvas_state.field_state().selected_entity(),
                None,
                "persisted UI {case}"
            );

            let resource = &app.spherical_presentation;
            let canvas_state = &app.spherical_canvas_state;
            let cache = &mut app.spherical_inspector_cache;
            resource.read_resource(|current| {
                let current = current.as_ref().unwrap();
                assert_eq!(current.state(), canvas_state.field_state(), "state {case}");
                assert_eq!(
                    current.state().selected_entity(),
                    None,
                    "publication {case}"
                );
                assert_eq!(
                    current.gpu_packet().source(),
                    current.source(),
                    "source {case}"
                );
                assert!(Arc::ptr_eq(
                    current.gpu_packet().layers_arc(),
                    current.layers_arc()
                ));
                assert_eq!(
                    current.gpu_packet().layers().revisions(),
                    current.revisions().2,
                    "revisions {case}"
                );
                let diagnostic_count = current.document().diagnostics_for_ui().len();
                assert_eq!(
                    cache
                        .model(
                            current,
                            canvas_state.field_state(),
                            canvas_state.view_mode()
                        )
                        .unwrap()
                        .entity(),
                    None,
                    "first inspector {case}"
                );
                assert_eq!(cache.probe_for_test(), (1, diagnostic_count));
                assert_eq!(
                    cache
                        .model(
                            current,
                            canvas_state.field_state(),
                            canvas_state.view_mode()
                        )
                        .unwrap()
                        .entity(),
                    None,
                    "second inspector {case}"
                );
                assert_eq!(
                    cache.probe_for_test(),
                    (1, diagnostic_count),
                    "static inspector must not rescan {case}"
                );
            });
        }
    }

    #[test]
    fn corrupt_present_storage_recovers_visibly_to_legacy_without_implicit_spherical_build() {
        let render_state = request_test_render_state();
        let mut valid = TemplateApp::default();
        valid.spherical_space_spec.target_cell_count = 162;
        let mut valid_storage = TestStorage::default();
        eframe::set_value(&mut valid_storage, eframe::APP_KEY, &valid);
        let valid_wire = eframe::Storage::get_string(&valid_storage, eframe::APP_KEY).unwrap();

        for (from, to) in [
            ("equal_earth_zoom:1.0", "equal_earth_zoom:-1.0"),
            ("vector_display_speed:1.0", "vector_display_speed:9.0"),
            ("radius:6371000.0", "radius:-1.0"),
            ("world_origin:SphericalV1", "world_origin:FutureSphericalV2"),
        ] {
            let corrupt = valid_wire.replacen(from, to, 1);
            assert_ne!(corrupt, valid_wire, "fixture must corrupt `{from}`");
            let mut storage = TestStorage::default();
            eframe::Storage::set_string(&mut storage, eframe::APP_KEY, corrupt);
            let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
            cc.storage = Some(&storage);
            cc.wgpu_render_state = Some(render_state.clone());
            let app = TemplateApp::new(&cc);
            assert_eq!(app.world_origin, PersistedWorldOrigin::LegacyPlanarV1);
            assert_eq!(
                app.active_runtime_graph(),
                Some(AppRuntimeGraph::LegacyPlanarFoundation)
            );
            assert!(app.legacy_planar_document.is_some());
            assert!(app.spherical_presentation.read_resource(Option::is_none));
            assert!(app
                .spherical_runtime_error
                .as_deref()
                .is_some_and(|message| message.contains("persisted")));
        }
    }

    #[test]
    fn explicit_legacy_regeneration_is_atomic_across_build_gpu_failure_and_success() {
        let render_state = request_test_render_state();
        let persisted = TemplateApp {
            world_origin: PersistedWorldOrigin::LegacyPlanarV1,
            ..TemplateApp::default()
        };
        let mut app = create_from_persisted(persisted, &render_state);
        let spatial_before = Arc::clone(
            &app.legacy_planar_document
                .as_ref()
                .expect("legacy runtime is published")
                .spatial,
        );
        let packet_before = app
            .field_display
            .read_resource(FieldDisplayResourceState::current_cloned)
            .unwrap();
        let runtime_stage_ids_before = app.active_runtime_stage_ids().unwrap().to_vec();

        app.spherical_space_spec.target_cell_count = 1;
        assert!(app.try_regenerate_as_spherical(&render_state).is_err());
        assert_eq!(app.world_origin, PersistedWorldOrigin::LegacyPlanarV1);
        assert_eq!(
            app.active_runtime_graph(),
            Some(AppRuntimeGraph::LegacyPlanarFoundation)
        );
        assert!(app.spherical_presentation.read_resource(Option::is_none));
        assert!(Arc::ptr_eq(
            &spatial_before,
            &app.legacy_planar_document.as_ref().unwrap().spatial
        ));
        assert!(Arc::ptr_eq(
            &packet_before,
            &app.field_display
                .read_resource(FieldDisplayResourceState::current_cloned)
                .unwrap()
        ));
        assert_eq!(
            app.active_runtime_stage_ids(),
            Some(runtime_stage_ids_before.as_slice())
        );

        app.spherical_space_spec.target_cell_count = 162;
        assert!(app
            .try_regenerate_as_spherical_with_failure(
                &render_state,
                MigrationFailurePoint::GpuPrepare,
            )
            .is_err());
        assert_eq!(app.world_origin, PersistedWorldOrigin::LegacyPlanarV1);
        assert_eq!(
            app.active_runtime_graph(),
            Some(AppRuntimeGraph::LegacyPlanarFoundation)
        );
        assert!(app.spherical_presentation.read_resource(Option::is_none));
        assert!(Arc::ptr_eq(
            &spatial_before,
            &app.legacy_planar_document.as_ref().unwrap().spatial
        ));
        assert!(Arc::ptr_eq(
            &packet_before,
            &app.field_display
                .read_resource(FieldDisplayResourceState::current_cloned)
                .unwrap()
        ));
        assert_eq!(
            app.active_runtime_stage_ids(),
            Some(runtime_stage_ids_before.as_slice())
        );

        app.try_regenerate_as_spherical(&render_state).unwrap();
        assert_eq!(app.world_origin, PersistedWorldOrigin::SphericalV1);
        assert_eq!(
            app.active_runtime_graph(),
            Some(AppRuntimeGraph::SphericalNaturalFoundation)
        );
        assert!(app.legacy_planar_document.is_none());
        assert!(app.field_renderer.is_none());
        assert!(app.spherical_presentation.read_resource(Option::is_some));
        assert!(app
            .active_runtime_stage_ids()
            .unwrap()
            .iter()
            .any(|stage| stage == "spatial.spherical-voronoi"));
    }

    #[test]
    fn existing_spherical_runtime_rebuilds_from_the_current_publication_lineage() {
        let render_state = request_test_render_state();
        let mut app = create_from_persisted(TemplateApp::default(), &render_state);
        let (publication_address, packet_before, source_before) =
            app.spherical_presentation.read_resource(|current| {
                let current = current.as_ref().unwrap();
                (
                    std::ptr::from_ref(current),
                    Arc::clone(current.gpu_packet_arc()),
                    current.source().clone(),
                )
            });
        app.world_seed = app.world_seed.wrapping_add(1);

        app.try_rebuild_spherical_world(&render_state).unwrap();
        let (address_after, packet_after, source_after) =
            app.spherical_presentation.read_resource(|current| {
                let current = current.as_ref().unwrap();
                (
                    std::ptr::from_ref(current),
                    Arc::clone(current.gpu_packet_arc()),
                    current.source().clone(),
                )
            });
        assert_eq!(publication_address, address_after);
        assert!(!Arc::ptr_eq(&packet_before, &packet_after));
        assert_ne!(source_after, source_before);
        assert_eq!(source_after.root_seed(), RootSeed::new(app.world_seed));
        assert_eq!(app.world_origin, PersistedWorldOrigin::SphericalV1);
        assert_eq!(
            app.active_runtime_graph(),
            Some(AppRuntimeGraph::SphericalNaturalFoundation)
        );

        app.spherical_space_spec.target_cell_count = 1;
        assert!(app.try_rebuild_spherical_world(&render_state).is_err());
        app.spherical_presentation.read_resource(|current| {
            let current = current.as_ref().unwrap();
            assert_eq!(publication_address, std::ptr::from_ref(current));
            assert!(Arc::ptr_eq(&packet_after, current.gpu_packet_arc()));
            assert_eq!(source_after, *current.source());
        });
        assert_eq!(app.world_origin, PersistedWorldOrigin::SphericalV1);

        app.spherical_space_spec.target_cell_count = 162;
        let revisions_before_invalid_relief = app
            .spherical_presentation
            .read_resource(|current| current.as_ref().unwrap().revisions());
        let uploads_before_invalid_relief = render_state
            .renderer
            .read()
            .callback_resources
            .get::<crate::gpu::spherical::SphericalFieldRenderer>()
            .unwrap()
            .upload_counters();
        app.relief_spec.target_land_fraction = f32::NAN;
        assert!(app.try_rebuild_spherical_world(&render_state).is_err());
        app.spherical_presentation.read_resource(|current| {
            let current = current.as_ref().unwrap();
            assert_eq!(publication_address, std::ptr::from_ref(current));
            assert!(Arc::ptr_eq(&packet_after, current.gpu_packet_arc()));
            assert_eq!(source_after, *current.source());
            assert_eq!(revisions_before_invalid_relief, current.revisions());
        });
        assert_eq!(
            uploads_before_invalid_relief,
            render_state
                .renderer
                .read()
                .callback_resources
                .get::<crate::gpu::spherical::SphericalFieldRenderer>()
                .unwrap()
                .upload_counters()
        );
    }

    #[test]
    fn persisted_and_rebuilt_land_targets_reach_the_current_spherical_publication() {
        fn published_land_fraction(app: &TemplateApp) -> (f64, Vec<u32>, f32) {
            app.spherical_presentation.read_resource(|current| {
                let current = current.as_ref().unwrap();
                let document = current
                    .document()
                    .natural_foundation()
                    .expect("unit-test worlds build on the legacy foundation chain");
                let surface = document.surface.snapshot();
                let relief = document.relief.snapshot();
                let land_area = surface
                    .cells()
                    .iter()
                    .zip(relief.land_ocean().raw_values())
                    .filter_map(|(cell, &kind)| (kind == 1).then_some(cell.area.get()))
                    .sum::<f64>();
                (
                    land_area / surface.total_cell_area().get(),
                    relief
                        .elevation_m()
                        .values()
                        .iter()
                        .map(|value| value.to_bits())
                        .collect(),
                    relief.sea_level_m(),
                )
            })
        }

        let render_state = request_test_render_state();
        let mut persisted = TemplateApp::default();
        persisted.spherical_space_spec.target_cell_count = 162;
        persisted.relief_spec.target_land_fraction = 0.55;
        let mut app = create_from_persisted(persisted, &render_state);

        let (initial_actual, initial_elevation, initial_sea_level) = published_land_fraction(&app);
        assert!((initial_actual - 0.55).abs() <= 0.02);

        app.relief_spec.target_land_fraction = 0.25;
        app.try_rebuild_spherical_world(&render_state).unwrap();
        let (rebuilt_actual, rebuilt_elevation, rebuilt_sea_level) = published_land_fraction(&app);
        assert!((rebuilt_actual - 0.25).abs() <= 0.02);
        assert_eq!(initial_elevation, rebuilt_elevation);
        assert!(initial_sea_level < rebuilt_sea_level);
    }

    fn wait_for_world_build(app: &mut TemplateApp) {
        let context = egui::Context::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        while app.world_build.is_some() {
            app.poll_world_build(&context);
            assert!(
                std::time::Instant::now() <= deadline,
                "asynchronous world build timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }

    #[test]
    fn failed_spherical_startup_is_visible_and_retries_standalone_without_planar_fallback() {
        let render_state = request_test_render_state();
        let mut persisted = TemplateApp::default();
        persisted.spherical_space_spec.target_cell_count = 162;
        persisted.tectonic_spec.plate_count = 0;
        let mut app = create_from_persisted(persisted, &render_state);

        assert_eq!(app.world_origin, PersistedWorldOrigin::SphericalV1);
        assert!(app.spherical_presentation.read_resource(Option::is_none));
        assert!(app.legacy_planar_document.is_none());
        assert_eq!(app.active_runtime_graph(), None);
        assert!(app.spherical_runtime_error.as_deref().is_some());

        app.tectonic_spec.plate_count = TectonicSpec::default().plate_count;
        app.apply_spherical_action(SphericalCanvasAction::RegenerateAsSpherical);
        wait_for_world_build(&mut app);
        assert!(app.spherical_runtime_error.is_none());
        assert_eq!(app.world_origin, PersistedWorldOrigin::SphericalV1);
        assert_eq!(
            app.active_runtime_graph(),
            Some(AppRuntimeGraph::SphericalNaturalFoundation)
        );
        assert!(app.spherical_presentation.read_resource(Option::is_some));
        assert!(app.legacy_planar_document.is_none());
        assert!(render_state
            .renderer
            .read()
            .callback_resources
            .get::<crate::gpu::spherical::SphericalFieldRenderer>()
            .is_some());
    }

    #[test]
    fn gpu_failed_spherical_startup_is_visible_and_retry_publishes_once() {
        let render_state = request_test_render_state();
        let mut app = TemplateApp::default();
        app.spherical_space_spec.target_cell_count = 162;
        app.render_state = Some(render_state.clone());

        assert!(app
            .try_start_spherical_world(&render_state, MigrationFailurePoint::GpuPrepare)
            .is_err());
        assert_eq!(app.world_origin, PersistedWorldOrigin::SphericalV1);
        assert!(app.spherical_runtime_error.as_deref().is_some());
        assert!(app.spherical_presentation.read_resource(Option::is_none));
        assert!(app.legacy_planar_document.is_none());
        assert!(render_state
            .renderer
            .read()
            .callback_resources
            .get::<crate::gpu::spherical::SphericalFieldRenderer>()
            .is_none());

        app.apply_spherical_action(SphericalCanvasAction::RegenerateAsSpherical);
        wait_for_world_build(&mut app);
        assert!(app.spherical_runtime_error.is_none());
        assert!(app.spherical_presentation.read_resource(Option::is_some));
        assert!(render_state
            .renderer
            .read()
            .callback_resources
            .get::<crate::gpu::spherical::SphericalFieldRenderer>()
            .is_some());
    }

    #[test]
    fn packet_changing_app_actions_queue_only_the_current_callback_in_the_same_frame() {
        for (origin, action) in [
            (
                PersistedWorldOrigin::SphericalV1,
                SphericalCanvasAction::SelectFill(land_ocean_field_id()),
            ),
            (
                PersistedWorldOrigin::SphericalV1,
                SphericalCanvasAction::SetCentralMeridianRadians(0.75),
            ),
            (
                PersistedWorldOrigin::SphericalV1,
                SphericalCanvasAction::RegenerateAsSpherical,
            ),
            (
                PersistedWorldOrigin::LegacyPlanarV1,
                SphericalCanvasAction::RegenerateAsSpherical,
            ),
        ] {
            let render_state = request_test_render_state();
            let persisted = TemplateApp {
                world_origin: origin,
                ..TemplateApp::default()
            };
            let mut app = create_from_persisted(persisted, &render_state);
            let context = egui::Context::default();
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    ..Default::default()
                },
                |context| app.show_active_canvas_after_actions(context, vec![action.clone()]),
            );
            assert_eq!(app.world_origin, PersistedWorldOrigin::SphericalV1);
            assert_eq!(
                output
                    .shapes
                    .iter()
                    .filter(|shape| matches!(shape.shape, egui::epaint::Shape::Callback(_)))
                    .count(),
                1,
                "{origin:?} action frame must queue one spherical callback"
            );
            let expected_source = app
                .spherical_presentation
                .read_resource(|current| current.as_ref().unwrap().source().clone());
            let immutable_before = app_spherical_immutable_upload_counts(&render_state);
            let jobs = context.tessellate(output.shapes, 1.0);
            render_app_callbacks(&render_state, &jobs);
            let renderer = render_state.renderer.read();
            let spherical = renderer
                .callback_resources
                .get::<crate::gpu::spherical::SphericalFieldRenderer>()
                .unwrap();
            assert_eq!(spherical.installed_source(), Some(&expected_source));
            assert_eq!(
                app_spherical_immutable_upload_counts(&render_state),
                immutable_before,
                "same-frame callback must reuse the action's current packet"
            );
        }
    }

    #[test]
    fn spherical_inspector_cache_skips_static_camera_view_phase_scans_and_retires_old_sources() {
        let render_state = request_test_render_state();
        let mut app = create_from_persisted(TemplateApp::default(), &render_state);
        let diagnostics: Vec<_> = (0..10_000)
            .map(|index| OwnedViewDiagnostic {
                severity: ViewDiagnosticSeverity::Info,
                code: format!("test.inspector-cache.{index}"),
                field_id: None,
                cell_id: Some(crate::world::CellId::from_raw(0)),
                message: "large diagnostic scan fixture".to_owned(),
            })
            .collect();
        let mut cache = SphericalInspectorCache::default();

        let inspect =
            |app: &TemplateApp, cache: &mut SphericalInspectorCache, mode: SphericalViewMode| {
                app.spherical_presentation.read_resource(|current| {
                    cache
                        .model_with_diagnostics_for_test(
                            current.as_ref().unwrap(),
                            app.spherical_canvas_state.field_state(),
                            mode,
                            &diagnostics,
                        )
                        .unwrap();
                });
            };

        inspect(&app, &mut cache, SphericalViewMode::Map);
        assert_eq!(cache.probe_for_test(), (1, diagnostics.len()));
        inspect(&app, &mut cache, SphericalViewMode::Map);
        assert_eq!(cache.probe_for_test(), (1, diagnostics.len()));

        app.apply_spherical_action(SphericalCanvasAction::PanMap {
            delta: [0.125, -0.25],
        });
        inspect(&app, &mut cache, SphericalViewMode::Map);
        app.apply_spherical_action(SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe));
        inspect(&app, &mut cache, SphericalViewMode::Globe);
        app.apply_spherical_action(SphericalCanvasAction::AdvanceVectorPhase {
            frame_delta_seconds: 0.25,
        });
        inspect(&app, &mut cache, SphericalViewMode::Globe);
        assert_eq!(
            cache.probe_for_test(),
            (1, diagnostics.len()),
            "camera, view, and phase-only frames must not rebuild or scan"
        );

        app.apply_spherical_action(SphericalCanvasAction::SelectEntity(Some(
            crate::view::SelectedSurfaceEntity::Cell(crate::world::CellId::from_raw(0)),
        )));
        inspect(&app, &mut cache, SphericalViewMode::Globe);
        assert_eq!(cache.probe_for_test(), (2, diagnostics.len() * 2));

        let (old_document, old_layers) = app.spherical_presentation.read_resource(|current| {
            let current = current.as_ref().unwrap();
            (
                Arc::downgrade(current.document_arc()),
                Arc::downgrade(current.layers_arc()),
            )
        });
        app.world_seed = app.world_seed.wrapping_add(1);
        app.try_rebuild_spherical_world(&render_state).unwrap();
        inspect(&app, &mut cache, SphericalViewMode::Globe);
        assert_eq!(cache.probe_for_test(), (3, diagnostics.len() * 3));
        assert!(old_document.upgrade().is_none());
        assert!(old_layers.upgrade().is_none());
    }

    #[test]
    fn edge_picking_uses_an_exact_logical_pixel_circle_for_maps_and_the_globe_limb() {
        let render_state = request_test_render_state();
        let mut app = create_from_persisted(TemplateApp::default(), &render_state);
        app.apply_spherical_action(SphericalCanvasAction::SelectOverlay(Some(
            boundary_strength_field_id(),
        )));
        let canvas_size = [1000.0, 600.0];

        for (kind, zoom) in [
            (SphericalProjectionKind::EqualEarth, 1.0),
            (SphericalProjectionKind::EqualEarth, 3.0),
            (SphericalProjectionKind::Equirectangular, 1.0),
            (SphericalProjectionKind::Equirectangular, 3.0),
        ] {
            app.apply_spherical_action(SphericalCanvasAction::SetViewMode(SphericalViewMode::Map));
            app.apply_spherical_action(SphericalCanvasAction::SetProjectionKind(kind));
            app.apply_spherical_action(SphericalCanvasAction::ResetMap);
            if zoom != 1.0 {
                app.apply_spherical_action(SphericalCanvasAction::ZoomMap {
                    factor: zoom,
                    anchor: [0.0, 0.0],
                });
            }
            let (edge, base, normal) = app.spherical_presentation.read_resource(|current| {
                map_edge_pick_fixture(
                    current.as_ref().unwrap(),
                    &app.spherical_canvas_state,
                    canvas_size,
                )
            });
            assert_exact_edge_pick_circle(
                &app,
                Some(edge),
                base,
                normal,
                canvas_size,
                &format!("{kind:?} zoom {zoom}"),
            );
        }

        app.apply_spherical_action(SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe));
        app.apply_spherical_action(SphericalCanvasAction::ResetGlobe);
        app.apply_spherical_action(SphericalCanvasAction::TrackballGlobe {
            start: [500.0, 300.0],
            end: [537.0, 323.0],
            canvas_size,
        });
        let (edge, base, normal) = app.spherical_presentation.read_resource(|current| {
            globe_limb_edge_pick_fixture(
                current.as_ref().unwrap(),
                app.spherical_canvas_state.globe_camera(),
                canvas_size,
            )
        });
        assert_exact_edge_pick_circle(&app, Some(edge), base, normal, canvas_size, "globe limb");
    }

    fn assert_exact_edge_pick_circle(
        app: &TemplateApp,
        expected_edge: Option<crate::world::EdgeId>,
        base: [f64; 2],
        normal: [f64; 2],
        canvas_size: [f64; 2],
        context: &str,
    ) {
        for pixels_per_point in [1.0, 2.5] {
            app.spherical_presentation.read_resource(|current| {
                let current = current.as_ref().unwrap();
                let offset = |distance: f64| {
                    [
                        base[0] + normal[0] * distance,
                        base[1] + normal[1] * distance,
                    ]
                };
                let near = app.spherical_canvas_state.pick_screen(
                    current,
                    offset(7.9),
                    canvas_size,
                    pixels_per_point,
                );
                if let Some(edge) = expected_edge {
                    assert_eq!(
                        near,
                        Some(crate::view::SelectedSurfaceEntity::Edge(edge)),
                        "7.9 logical pixels must hit: {context}, DPR {pixels_per_point}",
                    );
                } else {
                    assert!(
                        matches!(near, Some(crate::view::SelectedSurfaceEntity::Edge(_))),
                        "7.9 logical pixels must hit an incident limb edge: {context}, DPR {pixels_per_point}",
                    );
                }
                assert!(
                    matches!(
                        app.spherical_canvas_state.pick_screen(
                            current,
                            offset(8.1),
                            canvas_size,
                            pixels_per_point,
                        ),
                        Some(crate::view::SelectedSurfaceEntity::Cell(_))
                    ),
                    "8.1 logical pixels must fall back to a cell: {context}, DPR {pixels_per_point}",
                );
            });
        }
    }

    fn map_edge_pick_fixture(
        presentation: &PublishedSphericalPresentation,
        state: &crate::ui::spherical::SphericalCanvasState,
        canvas_size: [f64; 2],
    ) -> (crate::world::EdgeId, [f64; 2], [f64; 2]) {
        let PreparedSphericalOverlay::Edge(field) = presentation.layers().overlay().unwrap() else {
            panic!("edge picking fixture requires an edge overlay");
        };
        presentation
            .map()
            .edge_segments()
            .iter()
            .filter(|segment| field.raw_values()[segment.edge().raw() as usize] != 0)
            .find_map(|segment| {
                let start = map_point_to_screen(state, segment.start(), canvas_size);
                let end = map_point_to_screen(state, segment.end(), canvas_size);
                let (edge, base, normal) =
                    edge_pick_fixture_from_segment(segment.edge(), start, end, canvas_size, 80.0)?;
                [normal, [-normal[0], -normal[1]]]
                    .into_iter()
                    .find(|normal| {
                        [7.9, 8.1].into_iter().all(|distance| {
                            let screen = [
                                base[0] + normal[0] * distance,
                                base[1] + normal[1] * distance,
                            ];
                            map_screen_direction(state, screen, canvas_size)
                                .and_then(|direction| presentation.locator().locate_cell(direction))
                                .and_then(|cell| {
                                    presentation.document().surface_for_ui().cell_edges(cell)
                                })
                                .is_some_and(|edges| edges.contains(&edge))
                        })
                    })
                    .map(|normal| (edge, base, normal))
            })
            .expect("map fixture must contain a long visible non-zero edge")
    }

    fn map_point_to_screen(
        state: &crate::ui::spherical::SphericalCanvasState,
        point: crate::view::ProjectionPoint,
        canvas_size: [f64; 2],
    ) -> [f64; 2] {
        let projection = state.projection();
        let bounds = projection.bounds();
        let bounds_width = bounds.max_x() - bounds.min_x();
        let bounds_height = bounds.max_y() - bounds.min_y();
        let aspect = canvas_size[0] / canvas_size[1];
        let map_aspect = bounds_width / bounds_height;
        let (fit_x, fit_y) = if aspect >= map_aspect {
            (2.0 / (bounds_height * aspect), 2.0 / bounds_height)
        } else {
            (2.0 / bounds_width, 2.0 * aspect / bounds_width)
        };
        let zoom = state.map_camera().zoom(projection.kind());
        let pan = state.map_camera().pan(projection.kind());
        let center_x = (bounds.min_x() + bounds.max_x()) * 0.5;
        let center_y = (bounds.min_y() + bounds.max_y()) * 0.5;
        let ndc = [
            (point.x() - center_x) * fit_x * zoom + pan[0] * 2.0,
            (point.y() - center_y) * fit_y * zoom + pan[1] * 2.0,
        ];
        [
            (ndc[0] + 1.0) * canvas_size[0] * 0.5,
            (1.0 - ndc[1]) * canvas_size[1] * 0.5,
        ]
    }

    fn map_screen_direction(
        state: &crate::ui::spherical::SphericalCanvasState,
        screen: [f64; 2],
        canvas_size: [f64; 2],
    ) -> Option<crate::world::spatial::UnitVector3> {
        let projection = state.projection();
        let bounds = projection.bounds();
        let bounds_width = bounds.max_x() - bounds.min_x();
        let bounds_height = bounds.max_y() - bounds.min_y();
        let aspect = canvas_size[0] / canvas_size[1];
        let map_aspect = bounds_width / bounds_height;
        let (fit_x, fit_y) = if aspect >= map_aspect {
            (2.0 / (bounds_height * aspect), 2.0 / bounds_height)
        } else {
            (2.0 / bounds_width, 2.0 * aspect / bounds_width)
        };
        let zoom = state.map_camera().zoom(projection.kind());
        let pan = state.map_camera().pan(projection.kind());
        let ndc = [
            2.0 * screen[0] / canvas_size[0] - 1.0,
            1.0 - 2.0 * screen[1] / canvas_size[1],
        ];
        let point = crate::view::ProjectionPoint::new(
            (ndc[0] - 2.0 * pan[0]) / (fit_x * zoom) + (bounds.min_x() + bounds.max_x()) * 0.5,
            (ndc[1] - 2.0 * pan[1]) / (fit_y * zoom) + (bounds.min_y() + bounds.max_y()) * 0.5,
        );
        projection.inverse(point).ok()
    }

    fn globe_limb_edge_pick_fixture(
        presentation: &PublishedSphericalPresentation,
        camera: crate::view::GlobeCamera,
        canvas_size: [f64; 2],
    ) -> (crate::world::EdgeId, [f64; 2], [f64; 2]) {
        let PreparedSphericalOverlay::Edge(_field) = presentation.layers().overlay().unwrap()
        else {
            panic!("globe picking fixture requires an edge overlay");
        };
        let visible: Vec<_> = presentation
            .globe()
            .edge_segments()
            .iter()
            .filter_map(|segment| {
                camera
                    .project_visible_segment_to_screen(segment.start(), segment.end(), canvas_size)
                    .map(|[start, end]| (segment.edge(), start, end))
            })
            .collect();
        presentation
            .globe()
            .edge_segments()
            .iter()
            .filter_map(|segment| {
                let start = crate::world::spatial::UnitVector3::new(
                    f64::from(segment.start()[0]),
                    f64::from(segment.start()[1]),
                    f64::from(segment.start()[2]),
                )
                .ok()?;
                let end = crate::world::spatial::UnitVector3::new(
                    f64::from(segment.end()[0]),
                    f64::from(segment.end()[1]),
                    f64::from(segment.end()[2]),
                )
                .ok()?;
                if camera.is_front_facing(start) == camera.is_front_facing(end) {
                    return None;
                }
                let [start, end] = camera.project_visible_segment_to_screen(
                    segment.start(),
                    segment.end(),
                    canvas_size,
                )?;
                let delta = [end[0] - start[0], end[1] - start[1]];
                let length = delta[0].hypot(delta[1]);
                if length < 20.0 {
                    return None;
                }
                let normal = [-delta[1] / length, delta[0] / length];
                [0.2, 0.35, 0.5, 0.65, 0.8].into_iter().find_map(|along| {
                    let base = [start[0] + delta[0] * along, start[1] + delta[1] * along];
                    [normal, [-normal[0], -normal[1]]]
                        .into_iter()
                        .find_map(|normal| {
                            let nearest = |distance| {
                                let screen = [
                                    base[0] + normal[0] * distance,
                                    base[1] + normal[1] * distance,
                                ];
                                let cell = camera
                                    .screen_to_ray(screen, canvas_size)
                                    .and_then(crate::view::intersect_unit_sphere)
                                    .and_then(|hit| {
                                        presentation.locator().locate_cell(hit.direction())
                                    })?;
                                let incident =
                                    presentation.document().surface_for_ui().cell_edges(cell)?;
                                visible
                                    .iter()
                                    .filter(|(edge, _, _)| incident.contains(edge))
                                    .map(|(edge, start, end)| {
                                        (*edge, test_point_segment_distance(screen, *start, *end))
                                    })
                                    .min_by(|left, right| {
                                        left.1
                                            .total_cmp(&right.1)
                                            .then_with(|| left.0.cmp(&right.0))
                                    })
                            };
                            let near = nearest(7.9)?;
                            let far = nearest(8.1)?;
                            ((near.1 - 7.9).abs() < 1.0e-6 && far.1 > 8.0)
                                .then_some((near.0, base, normal))
                        })
                })
            })
            .next()
            .expect("globe limb fixture must isolate one exact 8px incident-edge boundary")
    }

    fn test_point_segment_distance(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> f64 {
        let delta = [end[0] - start[0], end[1] - start[1]];
        let length_squared = delta[0].mul_add(delta[0], delta[1] * delta[1]);
        let along = (((point[0] - start[0]) * delta[0] + (point[1] - start[1]) * delta[1])
            / length_squared)
            .clamp(0.0, 1.0);
        let closest = [start[0] + along * delta[0], start[1] + along * delta[1]];
        (point[0] - closest[0]).hypot(point[1] - closest[1])
    }

    fn edge_pick_fixture_from_segment(
        edge: crate::world::EdgeId,
        start: [f64; 2],
        end: [f64; 2],
        canvas_size: [f64; 2],
        minimum_length: f64,
    ) -> Option<(crate::world::EdgeId, [f64; 2], [f64; 2])> {
        let delta = [end[0] - start[0], end[1] - start[1]];
        let length = delta[0].hypot(delta[1]);
        let base = [(start[0] + end[0]) * 0.5, (start[1] + end[1]) * 0.5];
        let margin = 10.0;
        if length < minimum_length
            || base[0] < margin
            || base[0] > canvas_size[0] - margin
            || base[1] < margin
            || base[1] > canvas_size[1] - margin
        {
            return None;
        }
        Some((edge, base, [-delta[1] / length, delta[0] / length]))
    }

    fn app_spherical_immutable_upload_counts(
        render_state: &eframe::egui_wgpu::RenderState,
    ) -> [u64; 7] {
        let renderer = render_state.renderer.read();
        let counters = renderer
            .callback_resources
            .get::<crate::gpu::spherical::SphericalFieldRenderer>()
            .unwrap()
            .upload_counters();
        [
            counters.map_geometry,
            counters.globe_geometry,
            counters.fill_field,
            counters.diagnostics,
            counters.palettes,
            counters.map_overlay_instances,
            counters.globe_overlay_instances,
        ]
    }

    fn render_app_callbacks(
        render_state: &eframe::egui_wgpu::RenderState,
        jobs: &[egui::ClippedPrimitive],
    ) {
        use eframe::egui_wgpu::wgpu;

        let descriptor = eframe::egui_wgpu::ScreenDescriptor {
            size_in_pixels: [800, 600],
            pixels_per_point: 1.0,
        };
        let target = render_state
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Task 10 app callback lifecycle target"),
                size: wgpu::Extent3d {
                    width: 800,
                    height: 600,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: render_state.target_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            render_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Task 10 app callback lifecycle encoder"),
                });
        let mut renderer = render_state.renderer.write();
        renderer.update_buffers(
            &render_state.device,
            &render_state.queue,
            &mut encoder,
            jobs,
            &descriptor,
        );
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Task 10 app callback lifecycle pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            renderer.render(&mut pass, jobs, &descriptor);
        }
        drop(renderer);
        render_state.queue.submit([encoder.finish()]);
    }

    #[test]
    fn persisted_non_default_view_is_bound_to_initial_and_rebuilt_whole_publications() {
        let render_state = request_test_render_state();
        let mut persisted = TemplateApp::default();
        let state = &mut persisted.spherical_canvas_state;
        state
            .apply(SphericalCanvasAction::PanMap {
                delta: [0.125, -0.25],
            })
            .unwrap();
        state
            .apply(SphericalCanvasAction::ZoomMap {
                factor: 1.5,
                anchor: [0.0, 0.0],
            })
            .unwrap();
        state
            .apply(SphericalCanvasAction::SetProjectionKind(
                SphericalProjectionKind::Equirectangular,
            ))
            .unwrap();
        state
            .apply(SphericalCanvasAction::SetCentralMeridianRadians(0.75))
            .unwrap();
        state
            .apply(SphericalCanvasAction::PanMap {
                delta: [-0.5, 0.375],
            })
            .unwrap();
        state
            .apply(SphericalCanvasAction::ZoomMap {
                factor: 2.5,
                anchor: [0.0, 0.0],
            })
            .unwrap();
        state
            .apply(SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe))
            .unwrap();
        state
            .apply(SphericalCanvasAction::TrackballGlobe {
                start: [400.0, 300.0],
                end: [460.0, 260.0],
                canvas_size: [800.0, 600.0],
            })
            .unwrap();
        state
            .apply(SphericalCanvasAction::ZoomGlobe { factor: 3.0 })
            .unwrap();
        state
            .apply(SphericalCanvasAction::SelectOverlay(Some(
                preliminary_prevailing_wind_m_s_field_id(),
            )))
            .unwrap();
        state
            .apply(SphericalCanvasAction::SetVectorLod(VectorGlyphLod::Low))
            .unwrap();
        let expected_view = state.presentation_view_state();

        let mut app = create_from_persisted(persisted, &render_state);
        let pick_before = app.spherical_presentation.read_resource(|current| {
            let current = current.as_ref().unwrap();
            assert_eq!(current.view_state(), &expected_view);
            assert_eq!(current.map().projection(), expected_view.projection());
            assert_eq!(current.state().vector_view_zoom(), 3.0);
            assert_eq!(
                current.layers().glyph_lod_key(),
                crate::view::GlyphLodKey::Medium
            );
            assert!(Arc::ptr_eq(
                current.gpu_packet().layers_arc(),
                current.layers_arc()
            ));
            assert_eq!(
                app.spherical_canvas_state.presentation_view_state(),
                expected_view
            );
            app.spherical_canvas_state
                .pick_screen(current, [400.0, 300.0], [800.0, 600.0], 1.0)
        });
        assert!(pick_before.is_some());

        app.try_rebuild_spherical_world(&render_state).unwrap();
        app.spherical_presentation.read_resource(|current| {
            let current = current.as_ref().unwrap();
            assert_eq!(current.view_state(), &expected_view);
            assert_eq!(current.map().projection(), expected_view.projection());
            assert_eq!(current.state().vector_view_zoom(), 3.0);
            assert_eq!(
                current.layers().glyph_lod_key(),
                crate::view::GlyphLodKey::Medium
            );
            assert!(Arc::ptr_eq(
                current.gpu_packet().layers_arc(),
                current.layers_arc()
            ));
            assert_eq!(
                app.spherical_canvas_state.presentation_view_state(),
                expected_view
            );
            assert_eq!(
                app.spherical_canvas_state.pick_screen(
                    current,
                    [400.0, 300.0],
                    [800.0, 600.0],
                    1.0
                ),
                pick_before
            );
        });
    }

    #[test]
    fn public_legacy_regeneration_rejects_an_existing_spherical_publication_without_side_effects() {
        let render_state = request_test_render_state();
        let mut unpublished_spherical = TemplateApp::default();
        unpublished_spherical.spherical_space_spec.target_cell_count = 1;
        assert!(matches!(
            unpublished_spherical.try_regenerate_as_spherical(&render_state),
            Err(AppRuntimeError::InvalidSphericalRegenerationState {
                origin: PersistedWorldOrigin::SphericalV1,
                publication_present: false,
            })
        ));

        let mut app = create_from_persisted(TemplateApp::default(), &render_state);
        let (publication_address, packet_before, source_before, revisions_before) =
            app.spherical_presentation.read_resource(|current| {
                let current = current.as_ref().unwrap();
                (
                    std::ptr::from_ref(current),
                    Arc::clone(current.gpu_packet_arc()),
                    current.source().clone(),
                    current.revisions(),
                )
            });
        let counters_before = render_state
            .renderer
            .read()
            .callback_resources
            .get::<crate::gpu::spherical::SphericalFieldRenderer>()
            .unwrap()
            .upload_counters();
        let stage_ids_before = app.active_runtime_stage_ids.clone();

        assert!(matches!(
            app.try_regenerate_as_spherical(&render_state),
            Err(AppRuntimeError::InvalidSphericalRegenerationState {
                origin: PersistedWorldOrigin::SphericalV1,
                publication_present: true,
            })
        ));
        app.world_origin = PersistedWorldOrigin::LegacyPlanarV1;
        assert!(matches!(
            app.try_regenerate_as_spherical(&render_state),
            Err(AppRuntimeError::InvalidSphericalRegenerationState {
                origin: PersistedWorldOrigin::LegacyPlanarV1,
                publication_present: true,
            })
        ));
        app.world_origin = PersistedWorldOrigin::SphericalV1;

        assert_eq!(app.world_origin, PersistedWorldOrigin::SphericalV1);
        assert_eq!(app.active_runtime_stage_ids, stage_ids_before);
        app.spherical_presentation.read_resource(|current| {
            let current = current.as_ref().unwrap();
            assert_eq!(publication_address, std::ptr::from_ref(current));
            assert!(Arc::ptr_eq(&packet_before, current.gpu_packet_arc()));
            assert_eq!(source_before, *current.source());
            assert_eq!(revisions_before, current.revisions());
        });
        let counters_after = render_state
            .renderer
            .read()
            .callback_resources
            .get::<crate::gpu::spherical::SphericalFieldRenderer>()
            .unwrap()
            .upload_counters();
        assert_eq!(counters_before, counters_after);
    }

    #[test]
    fn default_application_persists_continents_with_missing_field_compatibility() {
        let app = TemplateApp::default();
        assert_eq!(app.formation_spec.preset, WorldFormationPreset::Continents);
        assert_eq!(app.relief_spec, ReliefSpec::default());

        let mut encoded = serde_json::to_value(&app).unwrap();
        encoded.as_object_mut().unwrap().remove("formation_spec");
        encoded.as_object_mut().unwrap().remove("relief_spec");
        let restored: TemplateApp = serde_json::from_value(encoded).unwrap();
        assert_eq!(
            restored.formation_spec.preset,
            WorldFormationPreset::Continents
        );
        assert_eq!(restored.relief_spec, ReliefSpec::default());
    }

    #[test]
    fn application_roundtrip_preserves_manual_land_target() {
        let mut app = TemplateApp::default();
        app.relief_spec.target_land_fraction = 0.55;
        app.relief_spec.sea_level_policy = SeaLevelPolicy::TargetLandFraction;
        app.relief_spec.water_inventory_ratio = 1.75;

        let restored: TemplateApp =
            serde_json::from_value(serde_json::to_value(&app).unwrap()).unwrap();

        assert_eq!(restored.relief_spec.target_land_fraction, 0.55);
        assert_eq!(
            restored.relief_spec.sea_level_policy,
            SeaLevelPolicy::TargetLandFraction
        );
        assert_eq!(restored.relief_spec.water_inventory_ratio, 1.75);
    }

    #[test]
    fn formation_driver_locks_the_derived_authoring_control_without_mutating_it() {
        let published = SphericalWorldAreaSummary::Formation(FormationAreaSummary::new(
            0.38,
            0.40,
            0.55,
            0.21,
            -80.0,
            SeaLevelPolicy::WaterInventory,
            1.0,
        ));
        let mut relief = ReliefSpec {
            target_land_fraction: 0.55,
            ..ReliefSpec::default()
        };

        let physical =
            formation_authoring_control_state(WorldPipeline::Formation, &relief, Some(published));
        assert!(!physical.land_fraction_enabled);
        assert!(physical.continental_fraction_enabled);
        assert_eq!(
            physical.displayed_land_fraction.to_bits(),
            0.21_f32.to_bits()
        );
        assert_eq!(relief.target_land_fraction, 0.55);

        relief.sea_level_policy = SeaLevelPolicy::TargetLandFraction;
        let target =
            formation_authoring_control_state(WorldPipeline::Formation, &relief, Some(published));
        assert!(target.land_fraction_enabled);
        assert!(!target.continental_fraction_enabled);
        assert_eq!(target.displayed_land_fraction, 0.55);

        let legacy = formation_authoring_control_state(
            WorldPipeline::LegacyFoundation,
            &relief,
            Some(published),
        );
        assert!(legacy.land_fraction_enabled);
        assert!(legacy.continental_fraction_enabled);
        assert_eq!(legacy.displayed_land_fraction, 0.55);
    }

    #[test]
    fn formation_summary_reports_implicit_water_and_non_blocking_hints() {
        fn collect_text(shape: &egui::epaint::Shape, output: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(text) => output.push(text.galley.text().to_owned()),
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_text(shape, output);
                    }
                }
                _ => {}
            }
        }

        let summary = FormationAreaSummary::new(
            0.38,
            0.50,
            0.60,
            0.59,
            -1_200.0,
            SeaLevelPolicy::TargetLandFraction,
            2.5,
        );
        let context = egui::Context::default();
        let output = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                show_formation_area_summary(ui, summary);
            });
        });
        let mut texts = Vec::new();
        for shape in &output.shapes {
            collect_text(&shape.shape, &mut texts);
        }

        assert!(texts.iter().any(|text| {
            text == "陆地面积：目标 60.0%｜实际 59.0%｜偏差 -1.0 个百分点"
        }));
        assert!(texts.iter().any(|text| text == "海水量 = 2.500 × 地球"));
        assert!(texts.iter().any(|text| text.contains("海水量超出建议带")));
        assert!(texts.iter().any(|text| text.contains("将露出洋底")));
    }

    #[test]
    fn quality_tier_roundtrips_and_defaults_to_draft() {
        assert_eq!(
            TemplateApp::default().formation_quality_profile,
            crate::world::natural::NaturalQualityProfile::Draft
        );
        let app = TemplateApp {
            formation_quality_profile: crate::world::natural::NaturalQualityProfile::Standard,
            ..Default::default()
        };
        let restored: TemplateApp =
            serde_json::from_value(serde_json::to_value(&app).unwrap()).unwrap();
        assert_eq!(
            restored.formation_quality_profile,
            crate::world::natural::NaturalQualityProfile::Standard
        );
    }

    #[test]
    fn formation_surface_cache_is_keyed_by_tier_and_radius() {
        use super::formation_surface_key_is_stale;
        use crate::world::natural::NaturalQualityProfile;
        let key = Some((NaturalQualityProfile::Draft, 6_371_000.0));
        assert!(!formation_surface_key_is_stale(
            key,
            NaturalQualityProfile::Draft,
            6_371_000.0
        ));
        assert!(formation_surface_key_is_stale(
            key,
            NaturalQualityProfile::Standard,
            6_371_000.0
        ));
        assert!(formation_surface_key_is_stale(
            key,
            NaturalQualityProfile::Draft,
            3_000_000.0
        ));
        assert!(formation_surface_key_is_stale(
            None,
            NaturalQualityProfile::Draft,
            6_371_000.0
        ));
    }

    #[test]
    fn spherical_author_ui_reports_requested_evolved_target_actual_delta_and_sea_level() {
        fn collect_text(shape: &egui::epaint::Shape, output: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(text) => output.push(text.galley.text().to_owned()),
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_text(shape, output);
                    }
                }
                _ => {}
            }
        }

        let render_state = request_test_render_state();
        let app = create_from_persisted(TemplateApp::default(), &render_state);
        let world_summary = app
            .spherical_presentation
            .read_resource(|current| current.as_ref().unwrap().document().area_summary());
        let context = egui::Context::default();
        let output = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                show_spherical_area_summary(ui, world_summary);
            });
        });
        let mut texts = Vec::new();
        for shape in &output.shapes {
            collect_text(&shape.shape, &mut texts);
        }

        let crate::app::SphericalWorldAreaSummary::NaturalFoundation(summary) = world_summary
        else {
            panic!("unit-test worlds build on the legacy foundation chain");
        };
        assert!(texts.iter().any(|text| text == "面积依从性"));
        assert!(texts.iter().any(|text| {
            text == &format!(
                "初始大陆地壳面积：作者 {:.1}%｜演化后 {:.1}%",
                summary.requested_initial_continental_crust_fraction() * 100.0,
                summary.evolved_continental_crust_fraction() * 100.0,
            )
        }));
        assert!(texts.iter().any(|text| {
            text == &format!(
                "陆地面积：目标 {:.1}%｜实际 {:.1}%｜偏差 {:+.1} 个百分点",
                summary.target_land_fraction() * 100.0,
                summary.actual_land_fraction() * 100.0,
                (summary.actual_land_fraction() - summary.target_land_fraction()) * 100.0,
            )
        }));
        assert!(texts
            .iter()
            .any(|text| { text == &format!("海平面：{:.1} m", summary.sea_level_m()) }));
    }

    #[test]
    fn named_preset_applies_recommendation_while_random_preserves_author_value() {
        let mut formation = WorldFormationSpec::default();
        let mut tectonic = TectonicSpec::default();
        let mut relief = ReliefSpec::default();
        for (preset, expected_crust, expected_land) in [
            (WorldFormationPreset::Continents, 0.38, 0.20),
            (WorldFormationPreset::Archipelago, 0.26, 0.22),
            (WorldFormationPreset::Supercontinent, 0.42, 0.17),
            (WorldFormationPreset::GreatIsland, 0.28, 0.23),
            (WorldFormationPreset::VolcanicIslands, 0.16, 0.16),
        ] {
            apply_formation_preset_selection(&mut formation, &mut tectonic, &mut relief, preset);
            assert_eq!(formation.preset, preset);
            assert_eq!(tectonic.continental_crust_fraction, expected_crust);
            assert_eq!(relief.target_land_fraction, expected_land);
            assert_eq!(relief.sea_level_policy, SeaLevelPolicy::WaterInventory);
        }

        tectonic.continental_crust_fraction = 0.33;
        relief.target_land_fraction = 0.47;
        apply_formation_preset_selection(
            &mut formation,
            &mut tectonic,
            &mut relief,
            WorldFormationPreset::Random,
        );
        assert_eq!(formation.preset, WorldFormationPreset::Random);
        assert_eq!(tectonic.continental_crust_fraction, 0.33);
        assert_eq!(relief.target_land_fraction, 0.47);
    }

    #[test]
    fn default_natural_specs_are_geological_and_semantic() {
        let world = default_world_spec(RootSeed::new(42));
        assert_eq!(world.space.width.get(), 20_000_000.0);
        assert_eq!(world.space.height.get(), 10_000_000.0);
        assert_eq!(world.space.target_cell_count, DEFAULT_TARGET_CELL_COUNT);
        assert_eq!(world.technology, TechnologyBaseline::PreIndustrialMedieval);
        assert_eq!(
            TectonicSpec::default(),
            TectonicSpec {
                schema_version: 1,
                plate_count: 12,
                continental_crust_fraction: 0.38,
                activity: TectonicActivity::Moderate,
            }
        );
        assert_eq!(
            GeologicSpec::default(),
            GeologicSpec {
                schema_version: 1,
                hotspot_count: 4,
                mantle_activity: MantleActivity::Moderate,
            }
        );
    }

    #[test]
    fn natural_build_supplies_the_exact_external_artifact_set() {
        let world = default_world_spec(RootSeed::new(7));
        let formation = WorldFormationSpec {
            preset: WorldFormationPreset::Archipelago,
            ..WorldFormationSpec::default()
        };
        let external: ExternalArtifacts = build_legacy_planar_natural_external_artifacts(
            &world,
            &formation,
            &TectonicSpec::default(),
            &GeologicSpec::default(),
        )
        .unwrap();
        assert_eq!(external.len(), 8);
        assert!(external.hash::<PlanarSpaceArtifact>().is_ok());
        assert!(external.hash::<TectonicSpecArtifact>().is_ok());
        assert!(external.hash::<GeologicSpecArtifact>().is_ok());
        assert!(external.hash::<ClimateSpecArtifact>().is_ok());
        assert!(external.hash::<HydroErosionSpecArtifact>().is_ok());
        assert!(external.hash::<WorldFormationSpecArtifact>().is_ok());
        assert!(external.hash::<RulePackSetArtifact>().is_ok());
        assert!(external.hash::<AuthorConstraintsArtifact>().is_ok());

        let mut expected = ExternalArtifacts::new();
        expected
            .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
            .unwrap();
        expected
            .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
            .unwrap();
        expected
            .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
            .unwrap();
        expected
            .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
            .unwrap();
        expected
            .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
            .unwrap();
        expected
            .insert(WorldFormationSpecArtifact::new(formation))
            .unwrap();
        assert_eq!(
            external.hash::<GeologicSpecArtifact>().unwrap(),
            expected.hash::<GeologicSpecArtifact>().unwrap()
        );
        assert_eq!(
            external.hash::<ClimateSpecArtifact>().unwrap(),
            expected.hash::<ClimateSpecArtifact>().unwrap()
        );
        assert_eq!(
            external.hash::<HydroErosionSpecArtifact>().unwrap(),
            expected.hash::<HydroErosionSpecArtifact>().unwrap()
        );
        assert_eq!(
            external.hash::<WorldFormationSpecArtifact>().unwrap(),
            expected.hash::<WorldFormationSpecArtifact>().unwrap()
        );
        assert_eq!(
            external.hash::<RulePackSetArtifact>().unwrap(),
            expected.hash::<RulePackSetArtifact>().unwrap()
        );
        assert_eq!(
            external.hash::<AuthorConstraintsArtifact>().unwrap(),
            expected.hash::<AuthorConstraintsArtifact>().unwrap()
        );
    }

    #[test]
    fn invalid_formation_spec_is_rejected_by_the_narrow_app_boundary() {
        let world = default_world_spec(RootSeed::new(7));
        let mut formation = WorldFormationSpec::default();
        formation.schema_version += 1;
        let result = build_legacy_planar_natural_external_artifacts(
            &world,
            &formation,
            &TectonicSpec::default(),
            &GeologicSpec::default(),
        );
        assert!(matches!(
            result,
            Err(NaturalWorldBuildError::WorldFormationSpec(_))
        ));
    }

    #[test]
    fn successful_candidate_extracts_all_natural_artifacts() {
        let mut app = TemplateApp::default();
        let mut world = default_world_spec(RootSeed::new(11));
        world.space.target_cell_count = 128;
        app.try_replace_legacy_planar_natural_world(&world, &TectonicSpec::default())
            .unwrap();

        let document = app
            .legacy_planar_document
            .as_ref()
            .expect("successful replacement publishes a document");
        assert_eq!(document.spatial.snapshot().cell_count(), 128);
        assert_eq!(
            document.formation.formation().requested(),
            WorldFormationPreset::Continents
        );
        assert_eq!(document.tectonic.snapshot().cell_count(), 128);
        assert_eq!(document.mantle.snapshot().cell_count(), 128);
        assert_eq!(document.relief.snapshot().cell_count(), 128);
        assert_eq!(document.geology.snapshot().cell_count(), 128);
        assert_eq!(document.climate.snapshot().cell_count(), 128);
        assert_eq!(document.hydro_erosion.snapshot().cell_count(), 128);
        let packet = app
            .field_display
            .read_resource(FieldDisplayResourceState::current_cloned)
            .unwrap();
        assert_eq!(packet.field().field_id(), &surface_elevation_m_field_id());
        assert_eq!(app.rule_build_summary.active_pack_count, 1);
        assert_eq!(app.rule_build_summary.author_constraint_count, 0);
        assert_eq!(app.rule_build_summary.satisfied_constraint_count, 0);
        assert_eq!(app.rule_build_summary.compromised_constraint_count, 0);
    }

    #[test]
    fn random_request_publishes_resolved_formation_provenance() {
        let mut app = TemplateApp::default();
        app.formation_spec.preset = WorldFormationPreset::Random;
        let mut world = default_world_spec(RootSeed::new(11));
        world.space.target_cell_count = 128;
        app.try_replace_legacy_planar_natural_world(&world, &TectonicSpec::default())
            .unwrap();

        let formation = app
            .legacy_planar_document
            .as_ref()
            .unwrap()
            .formation
            .formation();
        assert_eq!(formation.requested(), WorldFormationPreset::Random);
        assert!(matches!(
            formation.resolved(),
            ResolvedWorldFormationPreset::Continents
                | ResolvedWorldFormationPreset::Archipelago
                | ResolvedWorldFormationPreset::Supercontinent
                | ResolvedWorldFormationPreset::GreatIsland
                | ResolvedWorldFormationPreset::VolcanicIslands
        ));
        let provenance = formation_provenance_label(formation);
        assert!(provenance.contains("随机（按种子）"));
        assert!(provenance.contains('→'));
    }

    #[test]
    fn failed_candidate_preserves_last_complete_document_and_packet() {
        let mut app = TemplateApp::default();
        let mut valid = default_world_spec(RootSeed::new(13));
        valid.space.target_cell_count = 128;
        app.try_replace_legacy_planar_natural_world(&valid, &TectonicSpec::default())
            .unwrap();
        let spatial_before = app.legacy_planar_document.as_ref().unwrap().spatial.clone();
        let formation_before = app
            .legacy_planar_document
            .as_ref()
            .unwrap()
            .formation
            .clone();
        let tectonic_before = app
            .legacy_planar_document
            .as_ref()
            .unwrap()
            .tectonic
            .clone();
        let mantle_before = app.legacy_planar_document.as_ref().unwrap().mantle.clone();
        let relief_before = app.legacy_planar_document.as_ref().unwrap().relief.clone();
        let geology_before = app.legacy_planar_document.as_ref().unwrap().geology.clone();
        let climate_before = app.legacy_planar_document.as_ref().unwrap().climate.clone();
        let hydro_erosion_before = app
            .legacy_planar_document
            .as_ref()
            .unwrap()
            .hydro_erosion
            .clone();
        let packet_before = app
            .field_display
            .read_resource(FieldDisplayResourceState::current_cloned)
            .unwrap();
        let state_before = app.field_viewer_state.read_resource(Clone::clone);
        let summary_before = app.rule_build_summary;
        let mut expected_clock = app.display_revision_clock.clone();
        let expected_next_revision = expected_clock.issue().unwrap();

        let mut invalid = valid;
        invalid.space.target_cell_count = 1;
        assert!(app
            .try_replace_legacy_planar_natural_world(&invalid, &TectonicSpec::default())
            .is_err());

        assert!(Arc::ptr_eq(
            &spatial_before,
            &app.legacy_planar_document.as_ref().unwrap().spatial
        ));
        let document_after = app.legacy_planar_document.as_ref().unwrap();
        assert!(Arc::ptr_eq(&formation_before, &document_after.formation));
        assert!(Arc::ptr_eq(&tectonic_before, &document_after.tectonic));
        assert!(Arc::ptr_eq(&mantle_before, &document_after.mantle));
        assert!(Arc::ptr_eq(&relief_before, &document_after.relief));
        assert!(Arc::ptr_eq(&geology_before, &document_after.geology));
        assert!(Arc::ptr_eq(&climate_before, &document_after.climate));
        assert!(Arc::ptr_eq(
            &hydro_erosion_before,
            &document_after.hydro_erosion
        ));
        let packet_after = app
            .field_display
            .read_resource(FieldDisplayResourceState::current_cloned)
            .unwrap();
        assert!(Arc::ptr_eq(&packet_before, &packet_after));
        assert_eq!(
            app.field_viewer_state.read_resource(Clone::clone),
            state_before
        );
        assert_eq!(app.rule_build_summary, summary_before);
        let mut actual_clock = app.display_revision_clock.clone();
        assert_eq!(actual_clock.issue().unwrap(), expected_next_revision);
    }

    #[test]
    fn active_canvas_build_executes_the_legacy_planar_graph() {
        let mut world = default_world_spec(RootSeed::new(19));
        world.space.target_cell_count = 128;
        let candidate = super::build_legacy_planar_natural_candidate(
            &world,
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            &mut crate::engine::MemoryStageCache::new(),
            &crate::view::FieldDisplayState::default(),
            &crate::view::DisplayRevisionClock::default(),
        )
        .unwrap();

        assert!(candidate
            .report
            .stage_ids()
            .contains(&"spatial.planar-voronoi"));
        assert!(candidate
            .report
            .stage_ids()
            .iter()
            .all(|stage_id| !stage_id.starts_with("natural.spherical-")));
        assert_eq!(candidate.document.spatial.snapshot().cell_count(), 128);
        assert_eq!(candidate.document.tectonic.snapshot().cell_count(), 128);
        assert_eq!(candidate.document.mantle.snapshot().cell_count(), 128);
        assert_eq!(candidate.document.relief.snapshot().cell_count(), 128);
        assert_eq!(candidate.document.geology.snapshot().cell_count(), 128);
        assert_eq!(candidate.document.climate.snapshot().cell_count(), 128);
        assert_eq!(
            candidate.document.hydro_erosion.snapshot().cell_count(),
            128
        );
        assert_eq!(candidate.packet.mesh().cell_count(), 128);
    }

    #[test]
    fn rule_resolution_failure_preserves_document_packet_clock_and_summary() {
        let mut app = TemplateApp::default();
        let mut world = default_world_spec(RootSeed::new(17));
        world.space.target_cell_count = 128;
        app.try_replace_legacy_planar_natural_world(&world, &TectonicSpec::default())
            .unwrap();
        let spatial_before = app.legacy_planar_document.as_ref().unwrap().spatial.clone();
        let formation_before = app
            .legacy_planar_document
            .as_ref()
            .unwrap()
            .formation
            .clone();
        let tectonic_before = app
            .legacy_planar_document
            .as_ref()
            .unwrap()
            .tectonic
            .clone();
        let mantle_before = app.legacy_planar_document.as_ref().unwrap().mantle.clone();
        let relief_before = app.legacy_planar_document.as_ref().unwrap().relief.clone();
        let geology_before = app.legacy_planar_document.as_ref().unwrap().geology.clone();
        let climate_before = app.legacy_planar_document.as_ref().unwrap().climate.clone();
        let hydro_erosion_before = app
            .legacy_planar_document
            .as_ref()
            .unwrap()
            .hydro_erosion
            .clone();
        let packet_before = app
            .field_display
            .read_resource(FieldDisplayResourceState::current_cloned)
            .unwrap();
        let summary_before = app.rule_build_summary;
        let mut expected_clock = app.display_revision_clock.clone();
        let expected_next_revision = expected_clock.issue().unwrap();

        let pack_constraint = RulePack::new(
            RulePackId::new("sekai.test.low-plates").unwrap(),
            RuleVersion::new(1, 0, 0).unwrap(),
            RulePackKind::Ordinary,
            CoreSchemaRange::new(1, 1).unwrap(),
            Vec::new(),
            Vec::new(),
            vec![CapabilityContribution::TectonicConstraint(
                RuleTectonicConstraint::new(
                    RuleItemId::new("low-range").unwrap(),
                    ConstraintStrength::Hard,
                    TectonicConstraintClause::plate_count(2, 4).unwrap(),
                )
                .unwrap(),
            )],
        )
        .unwrap();
        let packs =
            RulePackSet::new(vec![earthlike_rule_pack().unwrap(), pack_constraint]).unwrap();
        let author_constraint = AuthorConstraint::new(
            AuthorObjectId::from_raw(7),
            ConstraintStrength::Hard,
            TectonicConstraintClause::plate_count(20, 24).unwrap(),
        )
        .unwrap();
        let authors =
            AuthorConstraints::new(AUTHOR_CONSTRAINTS_SCHEMA_V1, vec![author_constraint]).unwrap();

        assert!(app
            .try_replace_legacy_planar_natural_world_with_rule_inputs(
                &world,
                &TectonicSpec::default(),
                packs,
                authors,
            )
            .is_err());

        let document_after = app.legacy_planar_document.as_ref().unwrap();
        assert!(Arc::ptr_eq(&spatial_before, &document_after.spatial));
        assert!(Arc::ptr_eq(&formation_before, &document_after.formation));
        assert!(Arc::ptr_eq(&tectonic_before, &document_after.tectonic));
        assert!(Arc::ptr_eq(&mantle_before, &document_after.mantle));
        assert!(Arc::ptr_eq(&relief_before, &document_after.relief));
        assert!(Arc::ptr_eq(&geology_before, &document_after.geology));
        assert!(Arc::ptr_eq(&climate_before, &document_after.climate));
        assert!(Arc::ptr_eq(
            &hydro_erosion_before,
            &document_after.hydro_erosion
        ));
        let packet_after = app
            .field_display
            .read_resource(FieldDisplayResourceState::current_cloned)
            .unwrap();
        assert!(Arc::ptr_eq(&packet_before, &packet_after));
        let mut actual_clock = app.display_revision_clock.clone();
        assert_eq!(actual_clock.issue().unwrap(), expected_next_revision);
        assert_eq!(app.rule_build_summary, summary_before);
    }

    #[test]
    fn selected_hydro_field_survives_a_successful_rebuild() {
        use crate::ui::field::FieldControlAction;

        let mut app = TemplateApp::default();
        let mut first = default_world_spec(RootSeed::new(23));
        first.space.target_cell_count = 128;
        app.try_replace_legacy_planar_natural_world(&first, &TectonicSpec::default())
            .unwrap();
        let selected = surface_elevation_m_field_id();
        app.apply_field_control_action(FieldControlAction::SelectField(selected.clone()));
        assert_eq!(
            app.field_viewer_state
                .read_resource(|state| state.selected_field().cloned()),
            Some(selected.clone())
        );

        let mut second = first;
        second.root_seed = RootSeed::new(24);
        app.try_replace_legacy_planar_natural_world(&second, &TectonicSpec::default())
            .unwrap();
        assert_eq!(
            app.field_viewer_state
                .read_resource(|state| state.selected_field().cloned()),
            Some(selected)
        );
    }

    #[test]
    fn application_copy_describes_hydrology_without_implying_history_or_final_climate() {
        assert!(CURRENT_SLICE_STATUS_TEXT.contains("初步气候 → 水文/侵蚀"));
        assert!(CURRENT_SLICE_SUBTITLE.contains("当前时间切片（含水文与地表塑形）"));
        for copy in [CURRENT_SLICE_STATUS_TEXT, CURRENT_SLICE_SUBTITLE] {
            assert!(!copy.contains("历史时间线"));
            assert!(!copy.contains("最终气候"));
        }
        assert_ne!(
            preliminary_mean_air_temperature_c_field_id(),
            surface_elevation_m_field_id()
        );
    }
}
