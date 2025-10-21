use eframe::egui_wgpu::RenderState;
use egui::Rect;

use crate::{
    delaunay::{self, voronoi::generate_voronoi_render_data},
    gpu::{
        delaunay::delaunay_renderer::DelaunayRenderer,
        height_map::height_map_renderer::HeightMapRenderer, points_renderer::PointsRenderer,
        voronoi::voronoi_renderer::VoronoiRenderer,
    },
    resource::{
        CanvasStateResource, DelaunayRendererResource, HeightMapRendererResource,
        MapSystemResource, PointsRendererResource, VoronoiRendererResource,
    },
    terrain::HeightColorMap,
    ui::canvas::canvas::Canvas,
};

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct TemplateApp {
    // Example stuff:
    label: String,
    scene_rect: Rect,
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
    height_map_renderer: Option<HeightMapRendererResource>,
    #[serde(skip)] // This how you opt-out of serialization of a field
    canvas_state: CanvasStateResource,
    #[serde(skip)] // This how you opt-out of serialization of a field
    map_system: MapSystemResource,
}

impl Default for TemplateApp {
    fn default() -> Self {
        let canvas_resource = CanvasStateResource::default();
        Self {
            // Example stuff:
            label: "Hello World!".to_owned(),
            value: 2.7,
            scene_rect: Rect::ZERO,
            canvas_widget: Canvas::new(canvas_resource.clone()),
            points_renderer: None,
            delaunay_renderer: None,
            voronoi_renderer: None,
            height_map_renderer: None,
            canvas_state: canvas_resource,
            map_system: MapSystemResource::default(),
        }
    }
}

impl TemplateApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        let mut app = if let Some(storage) = cc.storage {
            let mut app: TemplateApp =
                eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
            app.points_renderer = None;
            app.delaunay_renderer = None;
            app.voronoi_renderer = None;
            app.height_map_renderer = None;
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
            let height_map_renderer_resource = app.create_height_map_renderer_resource(rs);

            app.points_renderer = Some(points_renderer_resource.clone());
            app.delaunay_renderer = Some(delaunay_renderer_resource.clone());
            app.voronoi_renderer = Some(voronoi_renderer_resource.clone());
            app.height_map_renderer = Some(height_map_renderer_resource.clone());
        }

        app
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

                // Terrain generation controls
                ui.separator();
                if ui.button("🎲 Regenerate Terrain").clicked() {
                    self.regenerate_terrain();
                }

                ui.separator();

                // Display current noise parameters
                self.map_system.read_resource(|map_system| {
                    ui.label(format!("Seed: {}", map_system.noise_config.seed));
                    ui.label(format!("Octaves: {}", map_system.noise_config.octaves));
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::widgets::global_theme_preference_buttons(ui);
                });
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's

            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                powered_by_egui_and_eframe(ui);
                egui::warn_if_debug_build(ui);
            });
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

    fn create_height_map_renderer_resource(
        &mut self,
        rs: &RenderState,
    ) -> HeightMapRendererResource {
        log::info!("create_height_map_renderer_resource");
        let mut height_map_renderer =
            HeightMapRenderer::new(&rs.device, rs.target_format, self.canvas_state.clone());

        // Get data from map system
        let (voronoi, heights) = self.map_system.read_resource(|map_system| {
            (map_system.voronoi.clone(), map_system.heights.clone())
        });

        // Create color map
        let color_map = HeightColorMap::earth_style();

        // Update geometry with height data
        height_map_renderer.update_geometry(&voronoi, &heights, &color_map);

        log::info!(
            "HeightMapRenderer: Initialized with {} vertices",
            height_map_renderer.vertex_count()
        );

        let height_map_renderer_resource = HeightMapRendererResource::new(height_map_renderer);

        // 注册到资源里，这样在回调里可以获取到
        rs.renderer
            .write()
            .callback_resources
            .insert::<HeightMapRendererResource>(height_map_renderer_resource.clone());

        height_map_renderer_resource
    }

    fn regenerate_terrain(&mut self) {
        log::info!("Regenerating terrain...");

        // Regenerate heights in map system
        self.map_system.with_resource(|map_system| {
            map_system.regenerate_heights();
        });

        // Update height map renderer with new data
        if let Some(height_map_renderer_resource) = &self.height_map_renderer {
            let (voronoi, heights) = self.map_system.read_resource(|map_system| {
                (map_system.voronoi.clone(), map_system.heights.clone())
            });

            let color_map = HeightColorMap::earth_style();

            height_map_renderer_resource.with_resource(|renderer| {
                renderer.update_geometry(&voronoi, &heights, &color_map);
                log::info!(
                    "HeightMapRenderer: Updated with {} vertices",
                    renderer.vertex_count()
                );
            });
        }
    }
}
