//! CPU bake of the T1 amplified view into one equirect RGBA8 texture.
//!
//! The bake shares its color truth with the cell view: the same
//! [`PaletteId::Hypsometric`] table and the same sea-anchored display radius
//! the document publishes, so both view modes read identically. The
//! longitude/latitude convention must match `sample_amplified` in
//! `assets/shaders/spherical_field.wgsl`.

use std::f64::consts::{PI, TAU};

use crate::generators::natural::{AmplificationLod, TerrainAmplifier};
use crate::view::{built_in_palette, sample_palette, PaletteId};
use crate::world::spatial::UnitVector3;

/// Sun direction for the baked hillshade, expressed in tangent (east, north,
/// up) components; roughly north-west, matching cartographic convention.
const HILLSHADE_LIGHT_TANGENT: [f64; 3] = [-0.55, 0.65, 0.75];
/// Slope-to-shade gain: metres of neighbour drop treated as unit slope.
const HILLSHADE_SLOPE_GAIN_M: f64 = 350.0;
/// Shade range so ridges brighten and valleys dim without crushing blacks.
const HILLSHADE_FLOOR: f64 = 0.45;
const HILLSHADE_SPAN: f64 = 0.75;

/// M1 bake budget from the plan: one 4096×2048 equirect texture.
pub(super) const AMPLIFIED_BAKE_WIDTH: u32 = 4096;
pub(super) const AMPLIFIED_BAKE_HEIGHT: u32 = 2048;

/// One baked amplified view ready for GPU upload.
pub(super) struct AmplifiedViewImage {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba8: Vec<u8>,
}

/// Bakes the amplified equirect color texture on the calling thread pool.
pub(super) fn bake_amplified_view(
    amplifier: &TerrainAmplifier,
    sea_level_m: f32,
    display_radius_m: f32,
    width: u32,
    height: u32,
) -> AmplifiedViewImage {
    let equator_footprint_m = TAU * amplifier.radius_m() / f64::from(width);
    let lod = AmplificationLod::for_sampling_footprint(
        amplifier.base_wavelength_m(),
        equator_footprint_m,
    );
    let w = width as usize;
    let h = height as usize;

    // Pass 1: heights, parallel over row bands.
    let mut heights = vec![0.0_f32; w * h];
    let threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4)
        .min(16);
    let rows_per_band = h.div_ceil(threads);
    std::thread::scope(|scope| {
        for (band_index, band) in heights.chunks_mut(rows_per_band * w).enumerate() {
            let first_row = band_index * rows_per_band;
            scope.spawn(move || {
                for (row_offset, row) in band.chunks_mut(w).enumerate() {
                    let y = first_row + row_offset;
                    let latitude = (0.5 - (y as f64 + 0.5) / h as f64) * PI;
                    let (sin_lat, cos_lat) = latitude.sin_cos();
                    for (x, height) in row.iter_mut().enumerate() {
                        let longitude = ((x as f64 + 0.5) / w as f64 - 0.5) * TAU;
                        let direction = direction_from_angles(cos_lat, sin_lat, longitude);
                        *height = amplifier.sample(direction, lod).elevation_m;
                    }
                }
            });
        }
    });

    // Pass 2: hillshaded hypsometric color.
    let palette = built_in_palette(PaletteId::Hypsometric);
    let sea = f64::from(sea_level_m);
    let radius = f64::from(display_radius_m.max(1.0));
    let light = normalized(HILLSHADE_LIGHT_TANGENT);
    let mut rgba8 = vec![0_u8; w * h * 4];
    std::thread::scope(|scope| {
        let heights = &heights;
        for (band_index, band) in rgba8.chunks_mut(rows_per_band * w * 4).enumerate() {
            let first_row = band_index * rows_per_band;
            scope.spawn(move || {
                for (row_offset, row) in band.chunks_mut(w * 4).enumerate() {
                    let y = first_row + row_offset;
                    for x in 0..w {
                        let index = y * w + x;
                        let elevation = f64::from(heights[index]);
                        let t = ((elevation - (sea - radius)) / (2.0 * radius)).clamp(0.0, 1.0);
                        let base = sample_palette(palette, t as f32);
                        let shade = hillshade(heights, w, h, x, y, light);
                        let pixel = &mut row[x * 4..x * 4 + 4];
                        pixel[0] = encode_srgb(f64::from(base.components()[0]) * shade);
                        pixel[1] = encode_srgb(f64::from(base.components()[1]) * shade);
                        pixel[2] = encode_srgb(f64::from(base.components()[2]) * shade);
                        pixel[3] = 255;
                    }
                }
            });
        }
    });

    AmplifiedViewImage {
        width,
        height,
        rgba8,
    }
}

fn direction_from_angles(cos_lat: f64, sin_lat: f64, longitude: f64) -> UnitVector3 {
    let (sin_lon, cos_lon) = longitude.sin_cos();
    UnitVector3::new(cos_lat * cos_lon, cos_lat * sin_lon, sin_lat)
        .expect("equirect angles produce unit directions")
}

/// Horn-style shade from central differences on the height grid; x wraps,
/// y clamps at the poles.
fn hillshade(heights: &[f32], w: usize, h: usize, x: usize, y: usize, light: [f64; 3]) -> f64 {
    let xm = (x + w - 1) % w;
    let xp = (x + 1) % w;
    let ym = y.saturating_sub(1);
    let yp = (y + 1).min(h - 1);
    let gx = (f64::from(heights[y * w + xp]) - f64::from(heights[y * w + xm]))
        / (2.0 * HILLSHADE_SLOPE_GAIN_M);
    let gy = (f64::from(heights[yp * w + x]) - f64::from(heights[ym * w + x]))
        / (2.0 * HILLSHADE_SLOPE_GAIN_M);
    let normal = normalized([-gx, gy, 1.0]);
    let dot = (normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]).max(0.0);
    HILLSHADE_FLOOR + HILLSHADE_SPAN * dot
}

fn normalized(vector: [f64; 3]) -> [f64; 3] {
    let norm = (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt();
    [vector[0] / norm, vector[1] / norm, vector[2] / norm]
}

fn encode_srgb(linear: f64) -> u8 {
    let clamped = linear.clamp(0.0, 1.0);
    let encoded = if clamped <= 0.003_130_8 {
        12.92 * clamped
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::natural::AmplificationFieldsView;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::SphericalOrogenyKind;
    use crate::world::{Meters, RootSeed, SphericalSpaceSpec};

    #[test]
    fn bake_is_deterministic_opaque_and_sized() {
        let surface = GeodesicVoronoiBuilder::build_cancellable(
            &SphericalSpaceSpec {
                radius: Meters::new(6_371_000.0).unwrap(),
                target_cell_count: 162,
            },
            || false,
        )
        .unwrap();
        let count = surface.cells().len();
        let elevation: Vec<f32> = surface
            .cells()
            .iter()
            .map(|cell| (2_000.0 * cell.centroid.components()[2]) as f32)
            .collect();
        let zeros = vec![0.0_f32; count];
        let ones = vec![1.0_f32; count];
        let kinds = vec![SphericalOrogenyKind::None; count];
        let fields = AmplificationFieldsView {
            final_elevation_m: &elevation,
            sea_level_m: 0.0,
            sediment_thickness_m: &zeros,
            erodibility: &zeros,
            annual_precipitation_mm: &ones,
            crust_age_myr: &zeros,
            lineation_east: &ones,
            lineation_north: &zeros,
            orogeny_kind: &kinds,
            orogeny_age_myr: &zeros,
        };
        let amplifier = TerrainAmplifier::new(&surface, fields, RootSeed::new(7)).unwrap();
        let first = bake_amplified_view(&amplifier, 0.0, 2_000.0, 64, 32);
        let second = bake_amplified_view(&amplifier, 0.0, 2_000.0, 64, 32);
        assert_eq!(first.rgba8, second.rgba8);
        assert_eq!(first.width, 64);
        assert_eq!(first.height, 32);
        assert_eq!(first.rgba8.len(), 64 * 32 * 4);
        assert!(first.rgba8.chunks_exact(4).all(|pixel| pixel[3] == 255));
    }
}
