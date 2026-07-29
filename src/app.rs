use std::sync::Arc;

use eframe::egui_wgpu::RenderState;
use egui::Rect;

mod legacy_display;

use legacy_display::{LegacyTerrainDisplay, LegacyTerrainDisplayAdapter};

use crate::{
    delaunay::{self, voronoi::generate_voronoi_render_data},
    gpu::{
        delaunay::delaunay_renderer::DelaunayRenderer, field::CellFieldRenderer,
        points_renderer::PointsRenderer, voronoi::voronoi_renderer::VoronoiRenderer,
    },
    resource::{
        CanvasStateResource, DelaunayRendererResource, FieldDisplayResource, FieldRendererResource,
        FieldViewerStateResource, MapSystemResource, PointsRendererResource,
        VoronoiRendererResource,
    },
    terrain::TerrainGenerator,
    ui::{
        canvas::canvas::Canvas,
        field::{show_field_controls, show_field_inspector, FieldControlAction},
    },
    view::{
        built_in_palette, prepare_cell_field, resolve_display_range, DisplayPrepareError,
        DisplayRevisionClock, DisplayRevisions, FieldCatalog, FieldDisplayState, LinearRgba,
        PaletteId, PreparedCellField, PreparedDiagnosticMask, PreparedFieldDisplay,
    },
    world::{fields::FieldPaletteHint, Meters, WorldPoint, WorldRect},
};

/// 可用的地形模板名称
const TEMPLATE_NAMES: [&str; 22] = [
    // 传统模板
    "Earth-like",
    "Archipelago",
    "Continental",
    "Volcanic Island",
    "Atoll",
    "Peninsula",
    "Highland",
    "Oceanic",
    // Azgaar 风格模板
    "Volcano",
    "High Island",
    "Low Island",
    "Continents",
    "Archipelago (Azgaar)",
    "Atoll (Azgaar)",
    "Mediterranean",
    "Peninsula (Azgaar)",
    "Pangea",
    "Isthmus",
    // 基于图元的新模板
    "Tectonic Collision",
    "Volcanic Archipelago",
    "Fjord Coast",
    "Rift Valley",
];

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct TemplateApp {
    // Example stuff:
    label: String,
    scene_rect: Rect,
    /// 当前选择的地形模板索引
    selected_template: usize,
    /// 随机种子
    terrain_seed: u64,
    /// 是否使用固定种子
    use_fixed_seed: bool,
    #[serde(skip)] // This how you opt-out of serialization of a field
    canvas_widget: Canvas,
    #[serde(skip)] // This how you opt-out of serialization of a field
    value: f32,
    #[serde(skip)] // This how you opt-out of serialization of a field
    points_renderer: Option<PointsRendererResource>,
    #[serde(skip)] // This how you opt-out of serialization of a field
    delaunay_renderer: Option<DelaunayRendererResource>,
    #[serde(skip)] // This how you opt-out of serialization of a field
    voronoi_renderer: Option<VoronoiRendererResource>,
    #[serde(skip)]
    field_renderer: Option<FieldRendererResource>,
    #[serde(skip)] // This how you opt-out of serialization of a field
    canvas_state: CanvasStateResource,
    #[serde(skip)] // This how you opt-out of serialization of a field
    map_system: MapSystemResource,
    #[serde(skip)]
    field_display: FieldDisplayResource,
    #[serde(skip)]
    field_viewer_state: FieldViewerStateResource,
    #[serde(skip)]
    legacy_display: Option<LegacyTerrainDisplay>,
    #[serde(skip)]
    last_plate_ids: Vec<u16>,
    #[serde(skip)]
    display_revision_clock: DisplayRevisionClock,
}

impl Default for TemplateApp {
    fn default() -> Self {
        let canvas_resource = CanvasStateResource::default();
        let map_system_resource = MapSystemResource::default();
        let field_display = FieldDisplayResource::default();
        let field_viewer_state = FieldViewerStateResource::default();
        Self {
            // Example stuff:
            label: "Hello World!".to_owned(),
            value: 2.7,
            scene_rect: Rect::ZERO,
            selected_template: 0, // 默认 Earth-like
            terrain_seed: 42,
            use_fixed_seed: false,
            canvas_widget: Canvas::new(
                canvas_resource.clone(),
                map_system_resource.clone(),
                field_display.clone(),
                field_viewer_state.clone(),
            ),
            points_renderer: None,
            delaunay_renderer: None,
            voronoi_renderer: None,
            field_renderer: None,
            canvas_state: canvas_resource,
            map_system: map_system_resource,
            field_display,
            field_viewer_state,
            legacy_display: None,
            last_plate_ids: Vec::new(),
            display_revision_clock: DisplayRevisionClock::default(),
        }
    }
}

impl TemplateApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 配置中文字体支持
        Self::setup_fonts(&cc.egui_ctx);

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        let mut app = if let Some(storage) = cc.storage {
            let mut app: TemplateApp =
                eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
            app.points_renderer = None;
            app.delaunay_renderer = None;
            app.voronoi_renderer = None;
            app.field_renderer = None;
            app
        } else {
            Default::default()
        };

        let wgpu_render_state = cc.wgpu_render_state.as_ref();
        if let Some(rs) = wgpu_render_state {
            // let device = &rs.device;

            // 构造我们的渲染器
            let points_renderer_resource = app.create_points_renderer_resource(rs);
            let delaunay_renderer_resource = app.create_delaunay_renderer_resource(rs);
            let voronoi_renderer_resource = app.create_voronoi_renderer_resource(rs);
            let field_renderer_resource = app.create_field_renderer_resource(rs);

            app.points_renderer = Some(points_renderer_resource.clone());
            app.delaunay_renderer = Some(delaunay_renderer_resource.clone());
            app.voronoi_renderer = Some(voronoi_renderer_resource.clone());
            app.field_renderer = Some(field_renderer_resource);

            // 生成初始地形
            app.generate_terrain();
        }

        app
    }

    /// 配置字体，支持中文显示
    fn setup_fonts(ctx: &egui::Context) {
        use egui::{FontData, FontDefinitions, FontFamily};

        let mut fonts = FontDefinitions::default();
        let noto_sans_sc = include_bytes!("../assets/fonts/NotoSansSC-Regular.otf");

        fonts.font_data.insert(
            "noto_sans_sc".to_owned(),
            std::sync::Arc::new(FontData::from_static(noto_sans_sc)),
        );

        // 将中文字体添加到所有字体族的首选列表
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

        #[cfg(debug_assertions)]
        {
            println!("Loaded bundled Chinese font: Noto Sans SC");
        }

        ctx.set_fonts(fonts);
    }
}

struct PreparedDisplayParts {
    field: Arc<PreparedCellField>,
    diagnostics: Arc<PreparedDiagnosticMask>,
    palette: Arc<[LinearRgba]>,
}

fn prepare_new_legacy_display(
    display: &LegacyTerrainDisplay,
    current_state: &FieldDisplayState,
    clock: &mut DisplayRevisionClock,
) -> Result<(FieldDisplayState, Arc<PreparedFieldDisplay>), DisplayPrepareError> {
    let catalog = FieldCatalog::from_extension_fields(&display.registry, &display.fields)?;
    let mut state = current_state.clone();
    state.reconcile(&catalog, display.mesh.cell_count());
    let parts = prepare_display_parts(display, &catalog, &state)?;
    let revisions = issue_all_revisions(clock)?;
    let packet = Arc::new(PreparedFieldDisplay::new(
        display.mesh.clone(),
        parts.field,
        parts.diagnostics,
        parts.palette,
        revisions,
        state.diagnostics_enabled(),
    )?);
    Ok((state, packet))
}

fn prepare_control_action(
    display: &LegacyTerrainDisplay,
    current: &PreparedFieldDisplay,
    state: &mut FieldDisplayState,
    clock: &mut DisplayRevisionClock,
    action: FieldControlAction,
) -> Result<Arc<PreparedFieldDisplay>, DisplayPrepareError> {
    let catalog = FieldCatalog::from_extension_fields(&display.registry, &display.fields)?;
    match action {
        FieldControlAction::InspectField(_) => {
            unreachable!("inspection actions are handled without rebuilding the packet")
        }
        FieldControlAction::SelectField(field) => {
            state.select_field(field);
            state.reconcile(&catalog, display.mesh.cell_count());
            let parts = prepare_display_parts(display, &catalog, state)?;
            rebuild_changed_packet(current, parts, state.diagnostics_enabled(), clock)
        }
        FieldControlAction::SetRangeMode(mode) => {
            state.set_range_mode(mode);
            let Some(view) = selected_field_view(&catalog, state) else {
                return Err(DisplayPrepareError::NoRenderableField);
            };
            if view.scalar_values().is_none() {
                return Ok(Arc::new(current.clone()));
            }
            let range = resolve_display_range(view, state.range_mode())?;
            Ok(Arc::new(current.with_display_range(range)))
        }
        FieldControlAction::SetPaletteOverride(palette) => {
            state.set_palette_override(palette);
            state.reconcile(&catalog, display.mesh.cell_count());
            let palette = prepare_palette(&catalog, state)?;
            let mut revisions = current.revisions();
            let palette = if current.palette() == palette.as_ref() {
                current.palette_arc().clone()
            } else {
                revisions.palette = clock.issue()?;
                palette
            };
            Ok(Arc::new(PreparedFieldDisplay::new(
                current.mesh_arc().clone(),
                current.field_arc().clone(),
                current.diagnostics_arc().clone(),
                palette,
                revisions,
                current.diagnostics_enabled(),
            )?))
        }
        FieldControlAction::SetDiagnosticsEnabled(enabled) => {
            state.set_diagnostics_enabled(enabled);
            Ok(Arc::new(current.with_diagnostics_enabled(enabled)))
        }
        FieldControlAction::SetDiagnosticScope(scope) => {
            state.set_diagnostic_scope(scope);
            let diagnostics = prepare_diagnostics(display, state)?;
            let mut revisions = current.revisions();
            let diagnostics = if current.diagnostics() == diagnostics.as_ref() {
                current.diagnostics_arc().clone()
            } else {
                revisions.diagnostics = clock.issue()?;
                diagnostics
            };
            Ok(Arc::new(PreparedFieldDisplay::new(
                current.mesh_arc().clone(),
                current.field_arc().clone(),
                diagnostics,
                current.palette_arc().clone(),
                revisions,
                current.diagnostics_enabled(),
            )?))
        }
    }
}

fn prepare_display_parts(
    display: &LegacyTerrainDisplay,
    catalog: &FieldCatalog<'_>,
    state: &FieldDisplayState,
) -> Result<PreparedDisplayParts, DisplayPrepareError> {
    let view = selected_field_view(catalog, state).ok_or(DisplayPrepareError::NoRenderableField)?;
    let field = Arc::new(prepare_cell_field(
        view,
        display.mesh.cell_count(),
        state.range_mode(),
    )?);
    let diagnostics = prepare_diagnostics(display, state)?;
    let palette = prepare_palette(catalog, state)?;
    Ok(PreparedDisplayParts {
        field,
        diagnostics,
        palette,
    })
}

fn selected_field_view<'catalog, 'data>(
    catalog: &'catalog FieldCatalog<'data>,
    state: &FieldDisplayState,
) -> Option<&'catalog crate::view::FieldView<'data>> {
    state
        .selected_field()
        .and_then(|field| catalog.get(field))
        .and_then(|entry| entry.view())
        .filter(|view| view.cell_fill_kind().is_ok())
}

fn prepare_diagnostics(
    display: &LegacyTerrainDisplay,
    state: &FieldDisplayState,
) -> Result<Arc<PreparedDiagnosticMask>, DisplayPrepareError> {
    Ok(Arc::new(PreparedDiagnosticMask::build(
        display.mesh.cell_count(),
        display
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.as_ref()),
        state.selected_field(),
        state.diagnostic_scope(),
    )?))
}

fn prepare_palette(
    catalog: &FieldCatalog<'_>,
    state: &FieldDisplayState,
) -> Result<Arc<[LinearRgba]>, DisplayPrepareError> {
    let schema = state
        .selected_field()
        .and_then(|field| catalog.get(field))
        .map(|entry| entry.schema())
        .ok_or(DisplayPrepareError::NoRenderableField)?;
    let schema_palette = match schema.display.palette() {
        FieldPaletteHint::Sequential => PaletteId::Sequential,
        FieldPaletteHint::Diverging => PaletteId::Diverging,
        FieldPaletteHint::Categorical => PaletteId::Categorical,
        FieldPaletteHint::Boolean | FieldPaletteHint::Vector => {
            return Err(DisplayPrepareError::UnsupportedCellFill {
                field: schema.id.clone(),
            });
        }
    };
    let palette = state.palette_override().unwrap_or(schema_palette);
    Ok(Arc::from(built_in_palette(palette)))
}

fn rebuild_changed_packet(
    current: &PreparedFieldDisplay,
    parts: PreparedDisplayParts,
    diagnostics_enabled: bool,
    clock: &mut DisplayRevisionClock,
) -> Result<Arc<PreparedFieldDisplay>, DisplayPrepareError> {
    let mut revisions = current.revisions();
    let field = if current.field() == parts.field.as_ref() {
        current.field_arc().clone()
    } else {
        revisions.field = clock.issue()?;
        parts.field
    };
    let diagnostics = if current.diagnostics() == parts.diagnostics.as_ref() {
        current.diagnostics_arc().clone()
    } else {
        revisions.diagnostics = clock.issue()?;
        parts.diagnostics
    };
    let palette = if current.palette() == parts.palette.as_ref() {
        current.palette_arc().clone()
    } else {
        revisions.palette = clock.issue()?;
        parts.palette
    };
    Ok(Arc::new(PreparedFieldDisplay::new(
        current.mesh_arc().clone(),
        field,
        diagnostics,
        palette,
        revisions,
        diagnostics_enabled,
    )?))
}

fn issue_all_revisions(
    clock: &mut DisplayRevisionClock,
) -> Result<DisplayRevisions, DisplayPrepareError> {
    Ok(DisplayRevisions::new(
        clock.issue()?,
        clock.issue()?,
        clock.issue()?,
        clock.issue()?,
    ))
}

impl eframe::App for TemplateApp {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Put your widgets into a `SidePanel`, `TopBottomPanel`, `CentralPanel`, `Window` or `Area`.
        // For inspiration and more examples, go to https://emilk.github.io/egui

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::menu::bar(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }

                egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's

            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                powered_by_egui_and_eframe(ui);
                egui::warn_if_debug_build(ui);
            });
        });

        let mut field_actions = Vec::new();

        // 左侧控制面板
        egui::SidePanel::left("control_panel")
            .resizable(true)
            .default_width(300.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("地图控制");

                    ui.separator();

                    // 地形模板选择
                    ui.label("地形模板:");
                    egui::ComboBox::from_label("")
                        .selected_text(TEMPLATE_NAMES[self.selected_template])
                        .show_ui(ui, |ui| {
                            for (i, name) in TEMPLATE_NAMES.iter().enumerate() {
                                ui.selectable_value(&mut self.selected_template, i, *name);
                            }
                        });

                    ui.add_space(8.0);

                    // 随机种子控制
                    ui.checkbox(&mut self.use_fixed_seed, "使用固定种子");
                    if self.use_fixed_seed {
                        ui.horizontal(|ui| {
                            ui.label("种子:");
                            ui.add(
                                egui::DragValue::new(&mut self.terrain_seed).range(0..=u64::MAX),
                            );
                        });
                    }

                    ui.add_space(8.0);

                    // 生成按钮
                    if ui.button("🗺 生成新地图").clicked() {
                        self.generate_terrain_with_template();
                    }

                    ui.separator();

                    // 图层可见性控制
                    ui.label("图层可见性:");
                    self.map_system.with_resource(|map_system| {
                        ui.checkbox(&mut map_system.layer_visibility.cell_fill, "字段填色");
                        ui.checkbox(&mut map_system.layer_visibility.voronoi_edges, "Voronoi边");
                        ui.checkbox(&mut map_system.layer_visibility.delaunay, "Delaunay三角");
                        ui.checkbox(&mut map_system.layer_visibility.points, "点");
                    });

                    ui.separator();

                    // 显示当前地形信息
                    ui.label("当前地形:");
                    ui.label(format!("模板: {}", TEMPLATE_NAMES[self.selected_template]));

                    if let Some(display) = &self.legacy_display {
                        ui.separator();
                        let catalog =
                            FieldCatalog::from_extension_fields(&display.registry, &display.fields)
                                .expect("the stored legacy display document is validated");
                        let state = self.field_viewer_state.read_resource(Clone::clone);
                        field_actions.extend(show_field_controls(ui, &catalog, &state));
                        ui.separator();
                        let diagnostics: Vec<_> = display
                            .diagnostics
                            .iter()
                            .map(|diagnostic| diagnostic.as_ref())
                            .collect();
                        show_field_inspector(ui, &catalog, &state, &diagnostics);
                        ui.small(format!("板块字段覆盖 {} 个单元", self.last_plate_ids.len()));
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

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(&mut self.canvas_widget);
        });
    }
}

fn powered_by_egui_and_eframe(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label("Powered by ");
        ui.hyperlink_to("egui", "https://github.com/emilk/egui");
        ui.label(" and ");
        ui.hyperlink_to(
            "eframe",
            "https://github.com/emilk/egui/tree/master/crates/eframe",
        );
        ui.label(".");
    });
}

impl TemplateApp {
    fn apply_field_control_action(&mut self, action: FieldControlAction) {
        if let FieldControlAction::InspectField(field) = action {
            let is_registered = self.legacy_display.as_ref().is_some_and(|display| {
                FieldCatalog::from_extension_fields(&display.registry, &display.fields)
                    .ok()
                    .is_some_and(|catalog| catalog.get(&field).is_some())
            });
            if is_registered {
                self.field_viewer_state
                    .with_resource(|state| state.inspect_field(field));
            }
            return;
        }

        let Some(display) = self.legacy_display.as_ref() else {
            return;
        };
        let Some(current) = self
            .field_display
            .read_resource(|resource| resource.current_cloned())
        else {
            return;
        };

        let mut next_state = self.field_viewer_state.read_resource(Clone::clone);
        let mut next_clock = self.display_revision_clock.clone();
        match prepare_control_action(display, &current, &mut next_state, &mut next_clock, action) {
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

    fn create_points_renderer_resource(&mut self, rs: &RenderState) -> PointsRendererResource {
        println!("create_points_renderer_resource");
        let mut points_renderer = PointsRenderer::new(&rs.device, rs.target_format);

        let points = self
            .map_system
            .read_resource(|map_system| map_system.grid.get_all_points().clone());
        points_renderer.update_points(points);

        let points_renderer_resource = PointsRendererResource::new(points_renderer);

        // 注册到资源里，这样在回调里可以获取到
        rs.renderer
            .write()
            .callback_resources
            .insert::<PointsRendererResource>(points_renderer_resource.clone());

        points_renderer_resource
    }

    fn create_delaunay_renderer_resource(&mut self, rs: &RenderState) -> DelaunayRendererResource {
        println!("create_delaunay_renderer_resource");
        let mut delaunay_renderer =
            DelaunayRenderer::new(&rs.device, rs.target_format, self.canvas_state.clone());
        let (indices, points) = self.map_system.read_resource(|map_system| {
            let points = map_system.grid.get_all_points();
            let indices = delaunay::triangulate(&points);
            (indices, points.clone())
        });
        // println!("triangles: {}", triangles.len());
        // let gpu_triangles = to_gpu_triangles(indices, &points);
        delaunay_renderer.update_points(points);
        delaunay_renderer.update_indices(indices);

        let delaunay_renderer_resource = DelaunayRendererResource::new(delaunay_renderer);

        // 注册到资源里，这样在回调里可以获取到
        rs.renderer
            .write()
            .callback_resources
            .insert::<DelaunayRendererResource>(delaunay_renderer_resource.clone());

        delaunay_renderer_resource
    }

    fn create_voronoi_renderer_resource(&mut self, rs: &RenderState) -> VoronoiRendererResource {
        println!("create_voronoi_renderer_resource");
        let mut voronoi_renderer =
            VoronoiRenderer::new(&rs.device, rs.target_format, self.canvas_state.clone());
        let (indices, points) = self.map_system.read_resource(|map_system| {
            let points = map_system.grid.get_all_points();
            let indices = delaunay::triangulate(&points);
            (indices, points.clone())
        });

        // 获取Voronoi索引化数据
        let (vertices, indices) = generate_voronoi_render_data(&indices, &points);
        voronoi_renderer.update_vertices(vertices);
        voronoi_renderer.update_indices(indices);

        let voronoi_renderer_resource = VoronoiRendererResource::new(voronoi_renderer);

        // 注册到资源里，这样在回调里可以获取到
        rs.renderer
            .write()
            .callback_resources
            .insert::<VoronoiRendererResource>(voronoi_renderer_resource.clone());

        voronoi_renderer_resource
    }

    fn create_field_renderer_resource(&mut self, rs: &RenderState) -> FieldRendererResource {
        let renderer = CellFieldRenderer::new(&rs.device, rs.target_format);
        let resource = FieldRendererResource::new(renderer);
        rs.renderer
            .write()
            .callback_resources
            .insert::<FieldRendererResource>(resource.clone());
        resource
    }

    /// 生成新的地形（使用选定的模板）
    fn generate_terrain_with_template(&mut self) {
        let template_name = TEMPLATE_NAMES[self.selected_template];
        let seed = if self.use_fixed_seed {
            self.terrain_seed
        } else {
            // 生成随机种子
            let new_seed = rand::random::<u64>();
            self.terrain_seed = new_seed;
            new_seed
        };

        println!(
            "Generating terrain with template '{}', seed: {}",
            template_name, seed
        );

        let adapted = self.map_system.read_resource(|map_system| {
            // 根据模板名称获取模板
            let template = match template_name {
                // 传统模板
                "Earth-like" => crate::terrain::TerrainTemplate::earth_like(),
                "Archipelago" => crate::terrain::TerrainTemplate::archipelago(),
                "Continental" => crate::terrain::TerrainTemplate::continental(),
                "Volcanic Island" => crate::terrain::TerrainTemplate::volcanic_island(),
                "Atoll" => crate::terrain::TerrainTemplate::atoll(),
                "Peninsula" => crate::terrain::TerrainTemplate::peninsula(),
                "Highland" => crate::terrain::TerrainTemplate::highland(),
                "Oceanic" => crate::terrain::TerrainTemplate::oceanic(),
                // Azgaar 风格模板
                "Volcano" => crate::terrain::TerrainTemplate::volcano(),
                "High Island" => crate::terrain::TerrainTemplate::high_island(),
                "Low Island" => crate::terrain::TerrainTemplate::low_island(),
                "Continents" => crate::terrain::TerrainTemplate::continents(),
                "Archipelago (Azgaar)" => crate::terrain::TerrainTemplate::archipelago_azgaar(),
                "Atoll (Azgaar)" => crate::terrain::TerrainTemplate::atoll_azgaar(),
                "Mediterranean" => crate::terrain::TerrainTemplate::mediterranean(),
                "Peninsula (Azgaar)" => crate::terrain::TerrainTemplate::peninsula_azgaar(),
                "Pangea" => crate::terrain::TerrainTemplate::pangea(),
                "Isthmus" => crate::terrain::TerrainTemplate::isthmus(),
                // 基于图元的新模板
                "Tectonic Collision" => crate::terrain::TerrainTemplate::tectonic_collision(),
                "Volcanic Archipelago" => crate::terrain::TerrainTemplate::volcanic_archipelago(),
                "Fjord Coast" => crate::terrain::TerrainTemplate::fjord_coast(),
                "Rift Valley" => crate::terrain::TerrainTemplate::rift_valley(),
                _ => crate::terrain::TerrainTemplate::earth_like(),
            };

            // 使用模板创建配置
            let config = crate::terrain::TerrainConfig::with_template_and_seed(template, seed);
            let generator = TerrainGenerator::new(config);

            // 获取单元格位置（Voronoi生成点）
            let cells = map_system.grid.get_all_points().clone();

            // 从Delaunay三角剖分提取邻居关系
            let neighbors = Self::extract_neighbors(&map_system.delaunay, cells.len());

            // 生成地形
            let (heights, _plates, plate_ids) = generator.generate(&cells, &neighbors);
            let zero = Meters::new(0.0).expect("zero is a finite world coordinate");
            let bounds = WorldRect::new(
                WorldPoint::new(zero, zero),
                WorldPoint::new(
                    Meters::new(f64::from(map_system.config.width))
                        .expect("u32 map width is finite"),
                    Meters::new(f64::from(map_system.config.height))
                        .expect("u32 map height is finite"),
                ),
            )
            .map_err(|error| error.to_string())?;
            let display = LegacyTerrainDisplayAdapter::build(
                bounds,
                &map_system.voronoi,
                &heights,
                &plate_ids,
            )
            .map_err(|error| error.to_string())?;
            Ok::<_, String>((heights, plate_ids, display))
        });

        let (heights, plate_ids, display) = match adapted {
            Ok(adapted) => adapted,
            Err(message) => {
                self.field_display.with_resource(|resource| {
                    resource
                        .reject_runtime("display.legacy_adapter", message)
                        .expect("the built-in display status code is valid");
                });
                return;
            }
        };

        let current_state = self.field_viewer_state.read_resource(Clone::clone);
        let mut next_clock = self.display_revision_clock.clone();
        let (next_state, packet) =
            match prepare_new_legacy_display(&display, &current_state, &mut next_clock) {
                Ok(candidate) => candidate,
                Err(error) => {
                    self.field_display
                        .with_resource(|resource| resource.reject_prepare(error));
                    return;
                }
            };

        self.map_system
            .with_resource(|map_system| map_system.cells_data.height = heights);
        self.last_plate_ids = plate_ids;
        self.legacy_display = Some(display);
        self.field_viewer_state
            .with_resource(|state| *state = next_state);
        self.field_display
            .with_resource(|resource| resource.replace(packet));
        self.display_revision_clock = next_clock;

        println!(
            "Terrain generated successfully with template '{}'!",
            template_name
        );
    }

    /// 生成新的地形（兼容旧代码）
    fn generate_terrain(&mut self) {
        self.generate_terrain_with_template();
    }

    /// 从Delaunay三角剖分提取每个点的邻居
    fn extract_neighbors(triangles: &[u32], num_points: usize) -> Vec<Vec<u32>> {
        use std::collections::HashSet;

        let mut neighbors: Vec<HashSet<u32>> = vec![HashSet::new(); num_points];

        // 遍历所有三角形
        for chunk in triangles.chunks(3) {
            if chunk.len() == 3 {
                let (a, b, c) = (chunk[0] as usize, chunk[1] as usize, chunk[2] as usize);

                // 每个点都是其他两个点的邻居
                neighbors[a].insert(chunk[1]);
                neighbors[a].insert(chunk[2]);
                neighbors[b].insert(chunk[0]);
                neighbors[b].insert(chunk[2]);
                neighbors[c].insert(chunk[0]);
                neighbors[c].insert(chunk[1]);
            }
        }

        // 转换为Vec<Vec<u32>>
        neighbors
            .into_iter()
            .map(|set| set.into_iter().collect())
            .collect()
    }
}
