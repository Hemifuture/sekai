use std::collections::HashMap;

use egui::{Color32, ColorImage, Context, TextureHandle, TextureId};

use crate::world::{Layer, LayerKind};

#[derive(Clone, Debug)]
pub struct PaletteStop {
    pub position: f32,
    pub color: Color32,
}

#[derive(Clone, Debug)]
pub struct Palette {
    stops: Vec<PaletteStop>,
}

impl Palette {
    pub fn new(mut stops: Vec<PaletteStop>) -> Self {
        stops.sort_by(|a, b| a.position.partial_cmp(&b.position).unwrap());
        Self { stops }
    }

    pub fn sample(&self, t: f32) -> Color32 {
        if self.stops.is_empty() {
            return Color32::WHITE;
        }
        let clamped = t.clamp(0.0, 1.0);
        for window in self.stops.windows(2) {
            if clamped >= window[0].position && clamped <= window[1].position {
                let range = (window[1].position - window[0].position).max(1e-5);
                let local_t = (clamped - window[0].position) / range;
                return lerp(window[0].color, window[1].color, local_t);
            }
        }
        if clamped <= self.stops[0].position {
            self.stops[0].color
        } else {
            self.stops.last().unwrap().color
        }
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let clamped = t.clamp(0.0, 1.0);
    let r = a.r() as f32 + (b.r() as f32 - a.r() as f32) * clamped;
    let g = a.g() as f32 + (b.g() as f32 - a.g() as f32) * clamped;
    let b = a.b() as f32 + (b.b() as f32 - a.b() as f32) * clamped;
    Color32::from_rgba_premultiplied(r.round() as u8, g.round() as u8, b.round() as u8, 255)
}

pub fn default_palettes() -> HashMap<LayerKind, Palette> {
    use LayerKind::*;
    let mut map = HashMap::new();
    map.insert(
        Elevation,
        Palette::new(vec![
            PaletteStop {
                position: 0.0,
                color: Color32::from_rgb(12, 31, 64),
            },
            PaletteStop {
                position: 0.35,
                color: Color32::from_rgb(40, 122, 184),
            },
            PaletteStop {
                position: 0.36,
                color: Color32::from_rgb(190, 184, 139),
            },
            PaletteStop {
                position: 0.6,
                color: Color32::from_rgb(90, 156, 78),
            },
            PaletteStop {
                position: 0.9,
                color: Color32::from_rgb(220, 220, 220),
            },
        ]),
    );
    map.insert(
        Erosion,
        Palette::new(vec![
            PaletteStop {
                position: 0.0,
                color: Color32::from_rgb(15, 6, 20),
            },
            PaletteStop {
                position: 1.0,
                color: Color32::from_rgb(196, 171, 125),
            },
        ]),
    );
    map.insert(
        Moisture,
        Palette::new(vec![
            PaletteStop {
                position: 0.0,
                color: Color32::from_rgb(165, 120, 60),
            },
            PaletteStop {
                position: 0.5,
                color: Color32::from_rgb(120, 180, 120),
            },
            PaletteStop {
                position: 1.0,
                color: Color32::from_rgb(40, 70, 140),
            },
        ]),
    );
    map.insert(
        Temperature,
        Palette::new(vec![
            PaletteStop {
                position: 0.0,
                color: Color32::from_rgb(10, 30, 96),
            },
            PaletteStop {
                position: 0.3,
                color: Color32::from_rgb(90, 140, 200),
            },
            PaletteStop {
                position: 0.7,
                color: Color32::from_rgb(200, 140, 60),
            },
            PaletteStop {
                position: 1.0,
                color: Color32::from_rgb(240, 40, 40),
            },
        ]),
    );
    map.insert(
        Rivers,
        Palette::new(vec![
            PaletteStop {
                position: 0.0,
                color: Color32::from_rgba_premultiplied(0, 0, 0, 0),
            },
            PaletteStop {
                position: 0.3,
                color: Color32::from_rgba_premultiplied(22, 80, 140, 80),
            },
            PaletteStop {
                position: 1.0,
                color: Color32::from_rgba_premultiplied(30, 120, 200, 200),
            },
        ]),
    );
    map.insert(
        Tectonics,
        Palette::new(vec![
            PaletteStop {
                position: 0.0,
                color: Color32::from_rgb(24, 32, 63),
            },
            PaletteStop {
                position: 1.0,
                color: Color32::from_rgb(240, 96, 96),
            },
        ]),
    );
    map
}

pub struct LayerRenderer {
    textures: HashMap<LayerKind, TextureHandle>,
    composite: Option<TextureHandle>,
}

impl LayerRenderer {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            composite: None,
        }
    }

    pub fn upload_layer(&mut self, ctx: &Context, layer: &Layer, palette: &Palette) -> TextureId {
        let image = colorize(layer, palette);
        let tex = ctx.load_texture(
            format!("layer_{:?}", layer.kind),
            image,
            egui::TextureOptions::LINEAR,
        );
        let id = tex.id();
        self.textures.insert(layer.kind, tex);
        id
    }

    pub fn upload_composite(
        &mut self,
        ctx: &Context,
        layers: &[(LayerKind, &Layer, &Palette, f32)],
    ) -> TextureId {
        if layers.is_empty() {
            self.composite = None;
            return TextureId::default();
        }
        let width = layers[0].1.width;
        let height = layers[0].1.height;
        let mut pixels = vec![Color32::TRANSPARENT; width * height];
        for (_kind, layer, palette, alpha) in layers {
            let colors = colorize(layer, palette);
            for (idx, color) in colors.pixels.iter().enumerate() {
                let existing = pixels[idx];
                let blended = blend(existing, *color, *alpha);
                pixels[idx] = blended;
            }
        }
        let image = ColorImage {
            size: [width, height],
            pixels,
        };
        let tex = ctx.load_texture("composite", image, egui::TextureOptions::LINEAR);
        let id = tex.id();
        self.composite = Some(tex);
        id
    }

    pub fn texture_for(&self, kind: LayerKind) -> Option<TextureId> {
        self.textures.get(&kind).map(|t| t.id())
    }

    pub fn composite_texture(&self) -> Option<TextureId> {
        self.composite.as_ref().map(|t| t.id())
    }
}

fn colorize(layer: &Layer, palette: &Palette) -> ColorImage {
    let mut pixels = Vec::with_capacity(layer.data.len());
    for &value in &layer.data {
        let color = palette.sample(value);
        pixels.push(color);
    }
    ColorImage {
        size: [layer.width, layer.height],
        pixels,
    }
}

fn blend(base: Color32, overlay: Color32, alpha: f32) -> Color32 {
    let a = (overlay.a() as f32 / 255.0) * alpha;
    let inv_a = 1.0 - a;
    let r = (base.r() as f32 * inv_a + overlay.r() as f32 * a).round() as u8;
    let g = (base.g() as f32 * inv_a + overlay.g() as f32 * a).round() as u8;
    let b = (base.b() as f32 * inv_a + overlay.b() as f32 * a).round() as u8;
    Color32::from_rgba_premultiplied(r, g, b, 255)
}
