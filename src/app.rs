use std::collections::HashMap;

use egui::{Align, Layout, TextureId};

use crate::rendering::{default_palettes, LayerRenderer, Palette};
use crate::world::{GenerationParameters, LayerKind, World};

const LAYER_ORDER: [LayerKind; 6] = [
    LayerKind::Elevation,
    LayerKind::Tectonics,
    LayerKind::Erosion,
    LayerKind::Rivers,
    LayerKind::Moisture,
    LayerKind::Temperature,
];

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct SekaiApp {
    seed: u64,
    params: GenerationParameters,
    show_composite: bool,
    active_layer: LayerKind,
    layer_alpha: HashMap<LayerKind, f32>,

    #[serde(skip)]
    world: World,
    #[serde(skip)]
    renderer: LayerRenderer,
    #[serde(skip)]
    palettes: HashMap<LayerKind, Palette>,
    #[serde(skip)]
    textures_dirty: bool,
}

impl Default for SekaiApp {
    fn default() -> Self {
        let params = GenerationParameters::default();
        let world = World::generate(1, &params);
        let renderer = LayerRenderer::new();
        let palettes = default_palettes();
        let mut layer_alpha = HashMap::new();
        for kind in LAYER_ORDER.iter() {
            layer_alpha.insert(*kind, 1.0);
        }
        Self {
            seed: 1,
            params,
            show_composite: true,
            active_layer: LayerKind::Elevation,
            layer_alpha,
            world,
            renderer,
            palettes,
            textures_dirty: true,
        }
    }
}

impl SekaiApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(storage) = cc.storage {
            if let Some(app) = eframe::get_value::<SekaiApp>(storage, eframe::APP_KEY) {
                return app;
            }
        }
        Default::default()
    }

    fn regenerate(&mut self) {
        self.world = World::generate(self.seed, &self.params);
        self.textures_dirty = true;
    }

    fn ensure_textures(&mut self, ctx: &egui::Context) -> egui::TextureId {
        if self.textures_dirty {
            for kind in LAYER_ORDER {
                if let (Some(layer), Some(palette)) =
                    (self.world.layer(kind), self.palettes.get(&kind))
                {
                    self.renderer.upload_layer(ctx, layer, palette);
                }
            }
            let layers: Vec<(LayerKind, &_, &_, f32)> = LAYER_ORDER
                .iter()
                .filter_map(|kind| {
                    let alpha = *self.layer_alpha.get(kind).unwrap_or(&1.0);
                    if !self.show_composite || alpha <= 0.01 {
                        return None;
                    }
                    self.world.layer(*kind).and_then(|layer| {
                        self.palettes
                            .get(kind)
                            .map(|palette| (*kind, layer, palette, alpha))
                    })
                })
                .collect();
            self.renderer.upload_composite(ctx, &layers);
            self.textures_dirty = false;
        }
        if self.show_composite {
            self.renderer
                .composite_texture()
                .unwrap_or_else(TextureId::default)
        } else {
            self.renderer
                .texture_for(self.active_layer)
                .unwrap_or_else(TextureId::default)
        }
    }
}

impl eframe::App for SekaiApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.heading("Sekai world generator");
            ui.label("Layer-aware GPU textures with procedural climate, rivers, and tectonics.");
        });

        egui::SidePanel::left("controls").show(ctx, |ui| {
            ui.heading("Generation");
            let mut params_changed = false;
            ui.horizontal(|ui| {
                ui.label("Seed");
                if ui
                    .add(egui::DragValue::new(&mut self.seed).speed(1))
                    .changed()
                {
                    self.textures_dirty = true;
                }
                if ui.button("Randomize").clicked() {
                    self.seed = rand::random();
                    self.regenerate();
                }
            });
            params_changed |= ui
                .add(egui::Slider::new(&mut self.params.sea_level, 0.05..=0.8).text("Sea level"))
                .changed();
            params_changed |= ui
                .add(egui::Slider::new(&mut self.params.rainfall, 0.1..=2.0).text("Rainfall"))
                .changed();
            params_changed |= ui
                .add(
                    egui::Slider::new(&mut self.params.erosion_strength, 0.2..=2.0)
                        .text("Erosion strength"),
                )
                .changed();
            params_changed |= ui
                .add(egui::Slider::new(&mut self.params.plate_count, 4..=16).text("Plate count"))
                .changed();
            params_changed |= ui
                .add(egui::Slider::new(&mut self.params.iterations, 8..=96).text("Iterations"))
                .changed();

            if ui.button("Regenerate world").clicked() {
                self.regenerate();
            } else if params_changed {
                self.regenerate();
            }

            ui.separator();
            ui.heading("Layers");
            ui.checkbox(&mut self.show_composite, "Composite view");
            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                for kind in LAYER_ORDER {
                    ui.horizontal(|ui| {
                        ui.label(format!("{:?}", kind));
                        if ui
                            .add(egui::Slider::new(
                                self.layer_alpha.entry(kind).or_insert(1.0),
                                0.0..=1.0,
                            ))
                            .changed()
                        {
                            self.textures_dirty = true;
                        }
                        if ui
                            .selectable_label(self.active_layer == kind, "Preview")
                            .clicked()
                        {
                            self.active_layer = kind;
                            self.show_composite = false;
                        }
                    });
                }
            });
        });

        let texture_id = self.ensure_textures(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(if self.show_composite {
                "Composite map"
            } else {
                "Single layer preview"
            });

            if texture_id == egui::TextureId::default() {
                ui.label("No textures available yet");
                return;
            }

            let available = ui.available_size();
            let size = [self.world.width as f32, self.world.height as f32];
            let aspect = size[0] / size[1];
            let target_width = available.x.min(available.y * aspect);
            let target_height = target_width / aspect;
            let image = egui::Image::new((texture_id, egui::vec2(target_width, target_height)));
            ui.add(image);

            ui.separator();
            ui.label(format!(
                "Resolution: {}x{} | Active layer: {:?}",
                size[0] as u32, size[1] as u32, self.active_layer
            ));
        });

        if ctx.input(|i| {
            i.key_pressed(egui::Key::R) && i.modifiers.matches_logically(egui::Modifiers::CTRL)
        }) {
            self.regenerate();
        }
    }
}
