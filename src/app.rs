use eframe::egui_wgpu::RenderState;
use egui::Rect;

use crate::{
    delaunay::{self, voronoi::generate_voronoi_render_data},
    gpu::{
        delaunay::delaunay_renderer::DelaunayRenderer, field::CellFieldRenderer,
        heightmap::heightmap_renderer::HeightmapRenderer, points_renderer::PointsRenderer,
        voronoi::voronoi_renderer::VoronoiRenderer,
    },
    resource::{
        CanvasStateResource, DelaunayRendererResource, FieldDisplayResource, FieldRendererResource,
        FieldViewerStateResource, HeightmapRendererResource, MapSystemResource,
        PointsRendererResource, VoronoiRendererResource,
    },
    terrain::TerrainGenerator,
    ui::canvas::canvas::Canvas,
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
    #[serde(skip)] // This how you opt-out of serialization of a field
    heightmap_renderer: Option<HeightmapRendererResource>,
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
            heightmap_renderer: None,
            field_renderer: None,
            canvas_state: canvas_resource,
            map_system: map_system_resource,
            field_display,
            field_viewer_state,
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
            app.heightmap_renderer = None;
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
            let heightmap_renderer_resource = app.create_heightmap_renderer_resource(rs);
            let field_renderer_resource = app.create_field_renderer_resource(rs);

            app.points_renderer = Some(points_renderer_resource.clone());
            app.delaunay_renderer = Some(delaunay_renderer_resource.clone());
            app.voronoi_renderer = Some(voronoi_renderer_resource.clone());
            app.heightmap_renderer = Some(heightmap_renderer_resource.clone());
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

        // 左侧控制面板
        egui::SidePanel::left("control_panel")
            .resizable(true)
            .default_width(250.0)
            .show(ctx, |ui| {
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
                        ui.add(egui::DragValue::new(&mut self.terrain_seed).range(0..=u64::MAX));
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
                    ui.checkbox(&mut map_system.layer_visibility.heightmap, "高度图");
                    ui.checkbox(&mut map_system.layer_visibility.voronoi_edges, "Voronoi边");
                    ui.checkbox(&mut map_system.layer_visibility.delaunay, "Delaunay三角");
                    ui.checkbox(&mut map_system.layer_visibility.points, "点");
                });

                ui.separator();

                // 显示当前地形信息
                ui.label("当前地形:");
                ui.label(format!("模板: {}", TEMPLATE_NAMES[self.selected_template]));
            });

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

    fn create_heightmap_renderer_resource(
        &mut self,
        rs: &RenderState,
    ) -> HeightmapRendererResource {
        println!("create_heightmap_renderer_resource");
        let heightmap_renderer = HeightmapRenderer::new(&rs.device, rs.target_format);

        let heightmap_renderer_resource = HeightmapRendererResource::new(heightmap_renderer);

        // 注册到资源里，这样在回调里可以获取到
        rs.renderer
            .write()
            .callback_resources
            .insert::<HeightmapRendererResource>(heightmap_renderer_resource.clone());

        heightmap_renderer_resource
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

        self.map_system.with_resource(|map_system| {
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
            let (heights, _plates, _plate_id) = generator.generate(&cells, &neighbors);

            // 更新高度数据
            map_system.cells_data.height = heights;

            println!(
                "Terrain generated successfully with template '{}'!",
                template_name
            );
        });
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
