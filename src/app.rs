use eframe::egui_wgpu::RenderState;
use egui::{accesskit::Point, Pos2, Rect, Widget};

use crate::{
    delaunay::{self, Triangle},
    gpu::{
        delaunay::{
            delaunay_renderer::{DelaunayRenderer, GPUTriangle},
            helpers::to_gpu_triangles,
        },
        map_renderer::MapRenderer,
        points_renderer::PointsRenderer,
    },
    resource::{
        CanvasStateResource, DelaunayRendererResource, MapRendererResource, PointsRendererResource,
    },
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
    canvas_state: CanvasStateResource,
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
            canvas_state: canvas_resource,
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
            app
        } else {
            Default::default()
        };

        let wgpu_render_state = cc.wgpu_render_state.as_ref();
        if let Some(rs) = wgpu_render_state {
            let device = &rs.device;

            // 构造我们的粒子系统
            let points_renderer_resource = create_points_renderer_resource(rs);
            let delaunay_renderer_resource = create_delaunay_renderer_resource(rs);

            app.points_renderer = Some(points_renderer_resource.clone());
            app.delaunay_renderer = Some(delaunay_renderer_resource.clone());
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

fn create_points_renderer_resource(rs: &RenderState) -> PointsRendererResource {
    let mut points_renderer = PointsRenderer::new(&rs.device, rs.target_format);
    points_renderer.update_points(vec![
        Pos2::new(0.1, 0.1),
        Pos2::new(100.0, 100.0),
        Pos2::new(150.0, 200.0),
    ]);

    let points_renderer_resource = PointsRendererResource::new(points_renderer);

    // 注册到资源里，这样在回调里可以获取到
    rs.renderer
        .write()
        .callback_resources
        .insert::<PointsRendererResource>(points_renderer_resource.clone());

    points_renderer_resource
}

fn create_delaunay_renderer_resource(rs: &RenderState) -> DelaunayRendererResource {
    // 创建一些离散点
    let points = vec![
        Pos2::new(0.1, 0.1),
        Pos2::new(-50.0, 100.0),
        Pos2::new(0.0, 200.0),
        Pos2::new(50.0, 300.0),
        Pos2::new(100.0, 400.0),
        Pos2::new(150.0, 500.0),
        Pos2::new(200.0, 600.0),
        Pos2::new(250.0, 700.0),
        Pos2::new(300.0, 800.0),
    ];
    let triangles = delaunay::triangulate(&points);
    let gpu_triangles = to_gpu_triangles(triangles);
    let mut delaunay_renderer = DelaunayRenderer::new(&rs.device, rs.target_format);
    delaunay_renderer.update_triangles(gpu_triangles);

    let delaunay_renderer_resource = DelaunayRendererResource::new(delaunay_renderer);

    // 注册到资源里，这样在回调里可以获取到
    rs.renderer
        .write()
        .callback_resources
        .insert::<DelaunayRendererResource>(delaunay_renderer_resource.clone());

    delaunay_renderer_resource
}
