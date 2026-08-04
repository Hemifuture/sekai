use std::sync::Arc;

use eframe::egui_wgpu::RenderState;
use thiserror::Error;

mod field_document;
#[cfg_attr(not(test), allow(dead_code))]
mod legacy_display;
mod natural_display;
mod natural_field_payloads;

use field_document::{prepare_control_action, prepare_new_document_display, FieldDocument};
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
    },
    rules::{
        default_rule_pack_set, AuthorConstraints, BuiltinRuleError, ConstraintAdoptionOutcome,
        ConstraintSource, RulePackSet, TectonicRuleResolution,
    },
    ui::{
        canvas::canvas::Canvas,
        field::{show_field_controls, show_field_inspector, FieldControlAction},
    },
    view::{DisplayPrepareError, DisplayRevisionClock, FieldDisplayState, PreparedFieldDisplay},
    world::{
        natural::{
            ClimateSpec, GeologicSpec, GeologicSpecError, HydroErosionSpec, NaturalSpecError,
            ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicActivity, TectonicSpec,
            WorldFormationPreset, WorldFormationSpec, WorldFormationSpecError,
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
const CURRENT_SLICE_STATUS_TEXT: &str =
    "当前切片：空间 → 板块/地壳 → 地形/地质 → 初步气候 → 水文/侵蚀";
const CURRENT_SLICE_SUBTITLE: &str = "前工业·中世纪幻想｜当前时间切片（含水文与地表塑形）";

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
    world_seed: u64,
    formation_spec: WorldFormationSpec,
    tectonic_spec: TectonicSpec,
    geologic_spec: GeologicSpec,
    #[serde(skip)]
    canvas_widget: Canvas,
    #[serde(skip)]
    field_renderer: Option<FieldRendererResource>,
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
}

impl Default for TemplateApp {
    fn default() -> Self {
        let canvas_state = CanvasStateResource::default();
        let field_display = FieldDisplayResource::default();
        let field_viewer_state = FieldViewerStateResource::default();
        Self {
            world_seed: 42,
            formation_spec: WorldFormationSpec::default(),
            tectonic_spec: TectonicSpec::default(),
            geologic_spec: GeologicSpec::default(),
            canvas_widget: Canvas::new(
                canvas_state,
                field_display.clone(),
                field_viewer_state.clone(),
            ),
            field_renderer: None,
            field_display,
            field_viewer_state,
            legacy_planar_document: None,
            stage_cache: MemoryStageCache::new(),
            display_revision_clock: DisplayRevisionClock::default(),
            rule_build_summary: RuleBuildSummary::default(),
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
        app.generate_legacy_planar_natural_world();
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
                ui.label(CURRENT_SLICE_STATUS_TEXT);
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
                            selected,
                        );
                    }
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
            self.generate_legacy_planar_natural_world();
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
        default_world_spec, formation_provenance_label, NaturalWorldBuildError, TemplateApp,
        CURRENT_SLICE_STATUS_TEXT, CURRENT_SLICE_SUBTITLE, DEFAULT_TARGET_CELL_COUNT,
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
    use crate::view::FieldDisplayResourceState;
    use crate::world::natural::{
        preliminary_mean_air_temperature_c_field_id, surface_elevation_m_field_id, ClimateSpec,
        GeologicSpec, HydroErosionSpec, MantleActivity, ResolvedWorldFormationPreset,
        TectonicActivity, TectonicSpec, WorldFormationPreset, WorldFormationSpec,
    };
    use crate::world::spatial::Topology;
    use crate::world::{AuthorObjectId, RootSeed, TechnologyBaseline};

    #[test]
    fn default_application_persists_continents_with_missing_field_compatibility() {
        let app = TemplateApp::default();
        assert_eq!(app.formation_spec.preset, WorldFormationPreset::Continents);

        let mut encoded = serde_json::to_value(&app).unwrap();
        encoded.as_object_mut().unwrap().remove("formation_spec");
        let restored: TemplateApp = serde_json::from_value(encoded).unwrap();
        assert_eq!(
            restored.formation_spec.preset,
            WorldFormationPreset::Continents
        );
    }

    #[test]
    fn named_preset_applies_recommendation_while_random_preserves_author_value() {
        let mut formation = WorldFormationSpec::default();
        let mut tectonic = TectonicSpec::default();
        for (preset, expected) in [
            (WorldFormationPreset::Continents, 0.38),
            (WorldFormationPreset::Archipelago, 0.26),
            (WorldFormationPreset::Supercontinent, 0.42),
            (WorldFormationPreset::GreatIsland, 0.28),
            (WorldFormationPreset::VolcanicIslands, 0.16),
        ] {
            apply_formation_preset_selection(&mut formation, &mut tectonic, preset);
            assert_eq!(formation.preset, preset);
            assert_eq!(tectonic.continental_crust_fraction, expected);
        }

        tectonic.continental_crust_fraction = 0.33;
        apply_formation_preset_selection(
            &mut formation,
            &mut tectonic,
            WorldFormationPreset::Random,
        );
        assert_eq!(formation.preset, WorldFormationPreset::Random);
        assert_eq!(tectonic.continental_crust_fraction, 0.33);
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
