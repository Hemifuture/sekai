use std::sync::Arc;

use eframe::egui_wgpu::RenderState;
use thiserror::Error;

mod field_document;
#[cfg_attr(not(test), allow(dead_code))]
mod legacy_display;
mod natural_display;

use field_document::{prepare_control_action, prepare_new_document_display, AppFieldDocument};
use natural_display::{NaturalDisplayError, NaturalFieldDocument};

use crate::world::spatial::Topology;
use crate::{
    engine::{
        ArtifactError, BuildEngine, BuildFailure, BuildReport, ExternalArtifacts, GraphError,
        MemoryStageCache,
    },
    generators::{
        natural::{
            natural_foundation_graph, ReliefArtifact, TectonicArtifact, TectonicSpecArtifact,
        },
        spatial::{PlanarSpaceArtifact, SpatialArtifact},
    },
    gpu::field::CellFieldRenderer,
    resource::{
        CanvasStateResource, FieldDisplayResource, FieldRendererResource, FieldViewerStateResource,
    },
    ui::{
        canvas::canvas::Canvas,
        field::{show_field_controls, show_field_inspector, FieldControlAction},
    },
    view::{DisplayPrepareError, DisplayRevisionClock, FieldDisplayState, PreparedFieldDisplay},
    world::{
        natural::{
            NaturalSpecError, TectonicActivity, TectonicSpec, MAX_CONTINENTAL_CRUST_FRACTION,
            MAX_PLATE_COUNT, MIN_CONTINENTAL_CRUST_FRACTION, MIN_PLATE_COUNT,
        },
        BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed, SpecError, TechnologyBaseline,
        WorldSpec, WORLD_SPEC_SCHEMA_V1,
    },
};

const DEFAULT_WORLD_WIDTH_M: f64 = 20_000_000.0;
const DEFAULT_WORLD_HEIGHT_M: f64 = 10_000_000.0;
const DEFAULT_TARGET_CELL_COUNT: u32 = 20_000;

/// Persisted UI state plus skipped runtime resources for the current natural slice.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct TemplateApp {
    world_seed: u64,
    tectonic_spec: TectonicSpec,
    #[serde(skip)]
    canvas_widget: Canvas,
    #[serde(skip)]
    field_renderer: Option<FieldRendererResource>,
    #[serde(skip)]
    field_display: FieldDisplayResource,
    #[serde(skip)]
    field_viewer_state: FieldViewerStateResource,
    #[serde(skip)]
    natural_document: Option<NaturalFieldDocument>,
    #[serde(skip)]
    stage_cache: MemoryStageCache,
    #[serde(skip)]
    display_revision_clock: DisplayRevisionClock,
}

impl Default for TemplateApp {
    fn default() -> Self {
        let canvas_state = CanvasStateResource::default();
        let field_display = FieldDisplayResource::default();
        let field_viewer_state = FieldViewerStateResource::default();
        Self {
            world_seed: 42,
            tectonic_spec: TectonicSpec::default(),
            canvas_widget: Canvas::new(
                canvas_state,
                field_display.clone(),
                field_viewer_state.clone(),
            ),
            field_renderer: None,
            field_display,
            field_viewer_state,
            natural_document: None,
            stage_cache: MemoryStageCache::new(),
            display_revision_clock: DisplayRevisionClock::default(),
        }
    }
}

impl TemplateApp {
    /// Creates the application, registers the sole active map renderer, and builds the first slice.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::setup_fonts(&cc.egui_ctx);

        let mut app = if let Some(storage) = cc.storage {
            let mut app: TemplateApp =
                eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
            app.field_renderer = None;
            app
        } else {
            Self::default()
        };

        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            app.field_renderer = Some(app.create_field_renderer_resource(render_state));
        }
        app.generate_natural_world();
        app
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

    fn generate_natural_world(&mut self) {
        let world = default_world_spec(RootSeed::new(self.world_seed));
        let tectonic = self.tectonic_spec.clone();
        if let Err(error) = self.try_replace_natural_world(&world, &tectonic) {
            log::error!("natural world build failed: {error}");
        }
    }

    fn try_replace_natural_world(
        &mut self,
        world: &WorldSpec,
        tectonic: &TectonicSpec,
    ) -> Result<(), NaturalWorldBuildError> {
        let current_state = self.field_viewer_state.read_resource(Clone::clone);
        let candidate = match build_natural_candidate(
            world,
            tectonic,
            &mut self.stage_cache,
            &current_state,
            &self.display_revision_clock,
        ) {
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

        let NaturalWorldCandidate {
            document,
            state,
            packet,
            clock,
            report,
        } = candidate;
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

        self.natural_document = Some(document);
        self.field_viewer_state
            .with_resource(|current| *current = state);
        self.field_display
            .with_resource(|resource| resource.replace(packet));
        self.display_revision_clock = clock;
        log::info!(
            "published natural slice: {cells} cells, {plates} plates, {segments} boundary segments"
        );
        Ok(())
    }

    fn apply_field_control_action(&mut self, action: FieldControlAction) {
        let Some(document) = self.natural_document.as_ref() else {
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
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("当前切片：空间 → 板块/地壳 → 地形");
                ui.separator();
                ui.hyperlink_to("egui", "https://github.com/emilk/egui");
                egui::warn_if_debug_build(ui);
            });
        });

        let mut field_actions = Vec::new();
        let mut rebuild = false;
        let mut new_seed = false;
        egui::SidePanel::left("control_panel")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("自然世界");
                    ui.label("前工业·中世纪幻想｜当前时间切片");
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label("根种子");
                        ui.add(egui::DragValue::new(&mut self.world_seed).range(0..=u64::MAX));
                    });
                    if ui.button("新种子并重建").clicked() {
                        new_seed = true;
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("板块数量");
                        ui.add(
                            egui::DragValue::new(&mut self.tectonic_spec.plate_count)
                                .range(MIN_PLATE_COUNT..=MAX_PLATE_COUNT),
                        );
                    });
                    ui.add(
                        egui::Slider::new(
                            &mut self.tectonic_spec.continental_crust_fraction,
                            MIN_CONTINENTAL_CRUST_FRACTION..=MAX_CONTINENTAL_CRUST_FRACTION,
                        )
                        .text("大陆地壳比例")
                        .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
                    );
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
                    if ui.button("按当前参数重建").clicked() {
                        rebuild = true;
                    }

                    if let Some(document) = self.natural_document.as_ref() {
                        ui.separator();
                        ui.label(format!(
                            "{} 个单元｜{} 个板块｜{} 条边界段",
                            document.spatial.snapshot().cell_count(),
                            document.tectonic.snapshot().plates().len(),
                            document.tectonic.snapshot().boundary_segments().len()
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
                    }

                    if let Some(status) = self
                        .field_display
                        .read_resource(|display| display.error().map(ToString::to_string))
                    {
                        ui.separator();
                        ui.colored_label(egui::Color32::LIGHT_RED, status);
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
            self.generate_natural_world();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(&mut self.canvas_widget);
        });
    }
}

fn activity_label(activity: TectonicActivity) -> &'static str {
    match activity {
        TectonicActivity::Quiet => "宁静",
        TectonicActivity::Moderate => "适中",
        TectonicActivity::Active => "活跃",
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

fn build_natural_external_artifacts(
    world: &WorldSpec,
    tectonic: &TectonicSpec,
) -> Result<ExternalArtifacts, NaturalWorldBuildError> {
    world.validate()?;
    tectonic.validate()?;
    let mut external = ExternalArtifacts::new();
    external.insert(PlanarSpaceArtifact::new(world.space.clone()))?;
    external.insert(TectonicSpecArtifact::new(tectonic.clone()))?;
    Ok(external)
}

fn build_natural_candidate(
    world: &WorldSpec,
    tectonic: &TectonicSpec,
    cache: &mut MemoryStageCache,
    current_state: &FieldDisplayState,
    clock: &DisplayRevisionClock,
) -> Result<NaturalWorldCandidate, NaturalWorldBuildError> {
    let external = build_natural_external_artifacts(world, tectonic)?;
    let outcome =
        BuildEngine::new(natural_foundation_graph()?).build(world.root_seed, external, cache)?;
    let spatial = outcome.artifacts.get::<SpatialArtifact>()?;
    let tectonic = outcome.artifacts.get::<TectonicArtifact>()?;
    let relief = outcome.artifacts.get::<ReliefArtifact>()?;
    let document = NaturalFieldDocument::build(spatial, tectonic, relief, &outcome.report)?;
    let mut next_clock = clock.clone();
    let (state, packet) = prepare_new_document_display(&document, current_state, &mut next_clock)?;
    Ok(NaturalWorldCandidate {
        document,
        state,
        packet,
        clock: next_clock,
        report: outcome.report,
    })
}

struct NaturalWorldCandidate {
    document: NaturalFieldDocument,
    state: FieldDisplayState,
    packet: Arc<PreparedFieldDisplay>,
    clock: DisplayRevisionClock,
    report: BuildReport,
}

#[derive(Debug, Error)]
enum NaturalWorldBuildError {
    #[error(transparent)]
    WorldSpec(#[from] SpecError),
    #[error(transparent)]
    TectonicSpec(#[from] NaturalSpecError),
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
        build_natural_external_artifacts, default_world_spec, TemplateApp,
        DEFAULT_TARGET_CELL_COUNT,
    };
    use crate::engine::ExternalArtifacts;
    use crate::generators::natural::TectonicSpecArtifact;
    use crate::generators::spatial::PlanarSpaceArtifact;
    use crate::view::FieldDisplayResourceState;
    use crate::world::natural::{elevation_field_id, TectonicActivity, TectonicSpec};
    use crate::world::spatial::Topology;
    use crate::world::{RootSeed, TechnologyBaseline};

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
    }

    #[test]
    fn natural_build_supplies_the_exact_external_artifact_set() {
        let world = default_world_spec(RootSeed::new(7));
        let external: ExternalArtifacts =
            build_natural_external_artifacts(&world, &TectonicSpec::default()).unwrap();
        assert_eq!(external.len(), 2);
        assert!(external.hash::<PlanarSpaceArtifact>().is_ok());
        assert!(external.hash::<TectonicSpecArtifact>().is_ok());
    }

    #[test]
    fn successful_candidate_extracts_all_natural_artifacts() {
        let mut app = TemplateApp::default();
        let mut world = default_world_spec(RootSeed::new(11));
        world.space.target_cell_count = 128;
        app.try_replace_natural_world(&world, &TectonicSpec::default())
            .unwrap();

        let document = app
            .natural_document
            .as_ref()
            .expect("successful replacement publishes a document");
        assert_eq!(document.spatial.snapshot().cell_count(), 128);
        assert_eq!(document.tectonic.snapshot().cell_count(), 128);
        assert_eq!(document.relief.snapshot().cell_count(), 128);
        let packet = app
            .field_display
            .read_resource(FieldDisplayResourceState::current_cloned)
            .unwrap();
        assert_eq!(packet.field().field_id(), &elevation_field_id());
    }

    #[test]
    fn failed_candidate_preserves_last_complete_document_and_packet() {
        let mut app = TemplateApp::default();
        let mut valid = default_world_spec(RootSeed::new(13));
        valid.space.target_cell_count = 128;
        app.try_replace_natural_world(&valid, &TectonicSpec::default())
            .unwrap();
        let spatial_before = app.natural_document.as_ref().unwrap().spatial.clone();
        let packet_before = app
            .field_display
            .read_resource(FieldDisplayResourceState::current_cloned)
            .unwrap();

        let mut invalid = valid;
        invalid.space.target_cell_count = 1;
        assert!(app
            .try_replace_natural_world(&invalid, &TectonicSpec::default())
            .is_err());

        assert!(Arc::ptr_eq(
            &spatial_before,
            &app.natural_document.as_ref().unwrap().spatial
        ));
        let packet_after = app
            .field_display
            .read_resource(FieldDisplayResourceState::current_cloned)
            .unwrap();
        assert!(Arc::ptr_eq(&packet_before, &packet_after));
    }

    #[test]
    fn default_application_source_has_no_legacy_generator_call_path() {
        let source = include_str!("app.rs");
        let old_generator = ["Terrain", "Generator"].concat();
        let old_entrypoint = ["generate_terrain_with", "_template"].concat();
        assert!(!source.contains(&old_generator));
        assert!(!source.contains(&old_entrypoint));
    }
}
