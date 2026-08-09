#![cfg_attr(not(test), allow(dead_code))]

use std::array;
use std::f64::consts::PI;

use noise::{NoiseFn, OpenSimplex};
use thiserror::Error;

use crate::generators::natural::relief_noise::FractalProfile;
use crate::world::spatial::SphericalSurfaceSnapshot;
use crate::world::CellId;

const MAX_OCTAVES: usize = 6;
const OCTAVE_SEED_STEP: u32 = 0x9E37_79B9;
const OCTAVE_ROTATION_3D: [[f64; 3]; 3] = [[0.36, 0.48, -0.8], [-0.8, 0.6, 0.0], [0.48, 0.64, 0.6]];
const MIN_CELL_DIAMETERS_PER_BAND: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::generators::natural) enum FieldShape {
    Smooth,
    Ridged,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::generators::natural) struct FieldBand {
    pub(in crate::generators::natural) angular_scale_rad: f64,
    pub(in crate::generators::natural) weight_milli: i32,
    pub(in crate::generators::natural) shape: FieldShape,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::generators::natural) struct FieldRecipe {
    pub(in crate::generators::natural) bands: &'static [FieldBand],
    pub(in crate::generators::natural) clamp_sigma_milli: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::generators::natural) struct QuantizedScalarField {
    values: Box<[i16]>,
}

impl QuantizedScalarField {
    pub(in crate::generators::natural) fn neutral(cell_count: usize) -> Self {
        Self {
            values: vec![0; cell_count].into_boxed_slice(),
        }
    }

    #[cfg(test)]
    pub(super) fn from_test_values(values: Vec<i16>) -> Self {
        Self {
            values: values.into_boxed_slice(),
        }
    }

    pub(in crate::generators::natural) fn get(&self, cell: CellId) -> Option<i16> {
        self.values.get(cell.raw() as usize).copied()
    }

    pub(in crate::generators::natural) fn values(&self) -> &[i16] {
        &self.values
    }

    pub(in crate::generators::natural) fn len(&self) -> usize {
        self.values.len()
    }

    pub(in crate::generators::natural) fn normalized_f64(&self, cell: CellId) -> f64 {
        self.get(cell)
            .map_or(0.0, |value| f64::from(value) / f64::from(i16::MAX))
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(in crate::generators::natural) enum MorphologyFieldError {
    #[error("a spherical morphology field requires at least one cell")]
    EmptySurface,
    #[error("a spherical morphology field recipe requires at least one band")]
    EmptyRecipe,
    #[error("field band {index} has invalid scale {angular_scale_rad} or weight {weight_milli}")]
    InvalidBand {
        index: usize,
        angular_scale_rad: f64,
        weight_milli: i32,
    },
    #[error("field clamp must be finite and positive, got {clamp_sigma_milli} milli-sigma")]
    InvalidClamp { clamp_sigma_milli: u16 },
    #[error("no field band is resolvable at median cell diameter {median_cell_diameter_rad} rad")]
    NoResolvableBand { median_cell_diameter_rad: f64 },
    #[error("field sample for cell {cell:?} is not finite")]
    NonFiniteSample { cell: CellId },
    #[error("field samples have zero or non-finite area-weighted variance")]
    DegenerateVariance,
}

/// The one deterministic coherent three-dimensional noise core shared by
/// spherical morphology and the existing spherical relief implementation.
pub(in crate::generators::natural) struct CoherentNoise3d {
    octaves: [OpenSimplex; MAX_OCTAVES],
}

impl CoherentNoise3d {
    pub(in crate::generators::natural) fn new(seed: u32) -> Self {
        Self {
            octaves: array::from_fn(|index| {
                OpenSimplex::new(seed.wrapping_add(OCTAVE_SEED_STEP.wrapping_mul(index as u32 + 1)))
            }),
        }
    }

    pub(super) fn sample(&self, point: [f64; 3]) -> f64 {
        self.octaves[0].get(point)
    }

    pub(in crate::generators::natural) fn fbm(
        &self,
        point: [f64; 3],
        profile: FractalProfile,
    ) -> f64 {
        self.fractal_sum(point, profile, |signal| signal)
            .clamp(-1.0, 1.0)
    }

    pub(in crate::generators::natural) fn ridged(
        &self,
        point: [f64; 3],
        profile: FractalProfile,
    ) -> f64 {
        self.fractal_sum(point, profile, |signal| {
            let ridge = 1.0 - signal.abs().clamp(0.0, 1.0);
            ridge * ridge
        })
        .clamp(0.0, 1.0)
    }

    fn fractal_sum(
        &self,
        point: [f64; 3],
        profile: FractalProfile,
        shape: impl Fn(f64) -> f64,
    ) -> f64 {
        profile.assert_valid();
        let mut coordinate = point.map(|component| component * profile.frequency);
        let mut amplitude = 1.0;
        let mut amplitude_sum = 0.0;
        let mut result = 0.0;
        for source in self.octaves.iter().take(profile.octaves) {
            result += shape(source.get(coordinate)) * amplitude;
            amplitude_sum += amplitude;
            amplitude *= profile.persistence;
            let rotated = OCTAVE_ROTATION_3D.map(|row| {
                row[0] * coordinate[0] + row[1] * coordinate[1] + row[2] * coordinate[2]
            });
            coordinate = rotated.map(|component| component * profile.lacunarity);
        }
        result / amplitude_sum
    }
}

pub(in crate::generators::natural) fn sample_spherical_field(
    surface: &SphericalSurfaceSnapshot,
    recipe: FieldRecipe,
    seed: u32,
) -> Result<QuantizedScalarField, MorphologyFieldError> {
    if surface.cells().is_empty() {
        return Err(MorphologyFieldError::EmptySurface);
    }
    if recipe.bands.is_empty() {
        return Err(MorphologyFieldError::EmptyRecipe);
    }
    if recipe.clamp_sigma_milli == 0 {
        return Err(MorphologyFieldError::InvalidClamp {
            clamp_sigma_milli: recipe.clamp_sigma_milli,
        });
    }
    for (index, band) in recipe.bands.iter().enumerate() {
        if !band.angular_scale_rad.is_finite()
            || band.angular_scale_rad <= 0.0
            || band.angular_scale_rad > PI
            || band.weight_milli <= 0
        {
            return Err(MorphologyFieldError::InvalidBand {
                index,
                angular_scale_rad: band.angular_scale_rad,
                weight_milli: band.weight_milli,
            });
        }
    }

    let median_cell_diameter_rad = median_equivalent_cell_diameter(surface);
    let retained = recipe
        .bands
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, band)| {
            band.angular_scale_rad >= MIN_CELL_DIAMETERS_PER_BAND * median_cell_diameter_rad
        })
        .collect::<Vec<_>>();
    if retained.is_empty() {
        return Err(MorphologyFieldError::NoResolvableBand {
            median_cell_diameter_rad,
        });
    }

    let samplers = retained
        .iter()
        .map(|(index, band)| {
            let band_seed = derive_band_seed(seed, *index, *band);
            let offset = seed_offset(band_seed);
            (*band, CoherentNoise3d::new(band_seed), offset)
        })
        .collect::<Vec<_>>();
    let weight_sum = retained
        .iter()
        .map(|(_, band)| f64::from(band.weight_milli))
        .sum::<f64>();

    let mut samples = Vec::with_capacity(surface.cells().len());
    for cell in surface.cells() {
        let point = cell.centroid.components();
        let mut combined = 0.0;
        for (band, sampler, offset) in &samplers {
            let coordinate =
                std::array::from_fn(|axis| point[axis] / band.angular_scale_rad + offset[axis]);
            let raw = sampler.sample(coordinate);
            let shaped = match band.shape {
                FieldShape::Smooth => raw,
                FieldShape::Ridged => {
                    let ridge = 1.0 - raw.abs().clamp(0.0, 1.0);
                    ridge * ridge
                }
            };
            combined += shaped * f64::from(band.weight_milli);
        }
        let sample = combined / weight_sum;
        if !sample.is_finite() {
            return Err(MorphologyFieldError::NonFiniteSample { cell: cell.id });
        }
        samples.push(sample);
    }

    quantize_area_normalized(surface, &samples, recipe.clamp_sigma_milli)
}

pub(in crate::generators::natural) fn sample_spherical_field_or_neutral(
    surface: &SphericalSurfaceSnapshot,
    recipe: FieldRecipe,
    seed: u32,
) -> Result<QuantizedScalarField, MorphologyFieldError> {
    match sample_spherical_field(surface, recipe, seed) {
        Err(MorphologyFieldError::NoResolvableBand { .. }) => {
            Ok(QuantizedScalarField::neutral(surface.cells().len()))
        }
        result => result,
    }
}

fn median_equivalent_cell_diameter(surface: &SphericalSurfaceSnapshot) -> f64 {
    let radius_squared = surface.radius().get().powi(2);
    let mut diameters = surface
        .cells()
        .iter()
        .map(|cell| {
            let unit_area = cell.area.get() / radius_squared;
            2.0 * (1.0 - unit_area / (2.0 * PI)).clamp(-1.0, 1.0).acos()
        })
        .collect::<Vec<_>>();
    diameters.sort_by(f64::total_cmp);
    diameters[diameters.len() / 2]
}

fn quantize_area_normalized(
    surface: &SphericalSurfaceSnapshot,
    samples: &[f64],
    clamp_sigma_milli: u16,
) -> Result<QuantizedScalarField, MorphologyFieldError> {
    let total_area = surface.total_cell_area().get();
    let mean = surface
        .cells()
        .iter()
        .zip(samples)
        .map(|(cell, sample)| cell.area.get() * sample)
        .sum::<f64>()
        / total_area;
    let variance = surface
        .cells()
        .iter()
        .zip(samples)
        .map(|(cell, sample)| cell.area.get() * (sample - mean).powi(2))
        .sum::<f64>()
        / total_area;
    if !variance.is_finite() || variance <= f64::EPSILON {
        return Err(MorphologyFieldError::DegenerateVariance);
    }

    let standard_deviation = variance.sqrt();
    let clamp_sigma = f64::from(clamp_sigma_milli) / 1_000.0;
    let values = samples
        .iter()
        .map(|sample| {
            (((sample - mean) / standard_deviation).clamp(-clamp_sigma, clamp_sigma) / clamp_sigma
                * f64::from(i16::MAX))
            .round() as i16
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Ok(QuantizedScalarField { values })
}

fn derive_band_seed(seed: u32, index: usize, band: FieldBand) -> u32 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai-spherical-field-band-v1\0");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&(index as u64).to_le_bytes());
    hasher.update(&band.angular_scale_rad.to_bits().to_le_bytes());
    hasher.update(&band.weight_milli.to_le_bytes());
    hasher.update(&[match band.shape {
        FieldShape::Smooth => 0,
        FieldShape::Ridged => 1,
    }]);
    u32::from_le_bytes(hasher.finalize().as_bytes()[..4].try_into().unwrap())
}

fn seed_offset(seed: u32) -> [f64; 3] {
    let mut state = u64::from(seed) | 1;
    std::array::from_fn(|_| {
        state = state
            .wrapping_add(0x9E37_79B9_7F4A_7C15)
            .wrapping_mul(0xBF58_476D_1CE4_E5B9);
        let mantissa = state >> 11;
        mantissa as f64 / ((1_u64 << 53) as f64) * 256.0 - 128.0
    })
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::{sample_spherical_field, FieldBand, FieldRecipe, FieldShape};
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::{CellId, Meters, SphericalSpaceSpec};

    const PLATE_RESISTANCE_BANDS: [FieldBand; 3] = [
        FieldBand {
            angular_scale_rad: 100.0_f64.to_radians(),
            weight_milli: 550,
            shape: FieldShape::Smooth,
        },
        FieldBand {
            angular_scale_rad: 42.0_f64.to_radians(),
            weight_milli: 300,
            shape: FieldShape::Smooth,
        },
        FieldBand {
            angular_scale_rad: 16.0_f64.to_radians(),
            weight_milli: 150,
            shape: FieldShape::Ridged,
        },
    ];
    const PLATE_RESISTANCE_RECIPE: FieldRecipe = FieldRecipe {
        bands: &PLATE_RESISTANCE_BANDS,
        clamp_sigma_milli: 3_000,
    };
    const MACRO_BANDS: [FieldBand; 1] = [FieldBand {
        angular_scale_rad: 120.0_f64.to_radians(),
        weight_milli: 1_000,
        shape: FieldShape::Smooth,
    }];
    const MACRO_PLUS_TINY_BANDS: [FieldBand; 2] = [
        MACRO_BANDS[0],
        FieldBand {
            angular_scale_rad: 0.1_f64.to_radians(),
            weight_milli: 500,
            shape: FieldShape::Ridged,
        },
    ];
    const MACRO_ONLY_RECIPE: FieldRecipe = FieldRecipe {
        bands: &MACRO_BANDS,
        clamp_sigma_milli: 3_000,
    };
    const MACRO_PLUS_TINY_DETAIL_RECIPE: FieldRecipe = FieldRecipe {
        bands: &MACRO_PLUS_TINY_BANDS,
        clamp_sigma_milli: 3_000,
    };

    fn test_sphere(target_cell_count: u32) -> crate::world::spatial::SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count,
        })
        .unwrap()
    }

    fn area_weighted_mean(
        sphere: &crate::world::spatial::SphericalSurfaceSnapshot,
        field: &super::QuantizedScalarField,
    ) -> f64 {
        let (weighted, area) = sphere.cells().iter().fold((0.0, 0.0), |acc, cell| {
            let value = field.normalized_f64(cell.id);
            (acc.0 + value * cell.area.get(), acc.1 + cell.area.get())
        });
        weighted / area
    }

    fn assert_cut_and_pole_neighbor_jumps_are_bounded(
        sphere: &crate::world::spatial::SphericalSurfaceSnapshot,
        field: &super::QuantizedScalarField,
    ) {
        let mut global_jumps = Vec::with_capacity(sphere.edges().len());
        let mut cut_jumps = Vec::new();
        let mut pole_jumps = Vec::new();
        for edge in sphere.edges() {
            let first = sphere.cell(edge.cells[0]).unwrap();
            let second = sphere.cell(edge.cells[1]).unwrap();
            let jump = (field.normalized_f64(first.id) - field.normalized_f64(second.id)).abs();
            global_jumps.push(jump);

            let first_xyz = first.centroid.components();
            let second_xyz = second.centroid.components();
            let first_lon = first_xyz[1].atan2(first_xyz[0]);
            let second_lon = second_xyz[1].atan2(second_xyz[0]);
            let crosses_cut = (first_lon - second_lon).abs() > PI;
            let near_pole = first_xyz[2].abs().max(second_xyz[2].abs()) > 0.90;
            if crosses_cut {
                cut_jumps.push(jump);
            }
            if near_pole {
                pole_jumps.push(jump);
            }
        }
        assert!(!cut_jumps.is_empty());
        assert!(!pole_jumps.is_empty());
        global_jumps.sort_by(f64::total_cmp);
        cut_jumps.sort_by(f64::total_cmp);
        pole_jumps.sort_by(f64::total_cmp);
        let p95 = global_jumps[global_jumps.len() * 95 / 100];
        let global_median = global_jumps[global_jumps.len() / 2];
        let cut_median = cut_jumps[cut_jumps.len() / 2];
        let cut_p95 = cut_jumps[cut_jumps.len() * 95 / 100];
        let pole_median = pole_jumps[pole_jumps.len() / 2];
        let pole_p95 = pole_jumps[pole_jumps.len() * 95 / 100];
        assert!(cut_median <= global_median * 1.5);
        assert!(pole_median <= global_median * 1.5);
        assert!(cut_p95 <= p95 * 1.35);
        assert!(pole_p95 <= p95 * 1.35);
    }

    #[test]
    fn spherical_field_is_area_centered_seeded_and_seam_free() {
        let sphere = test_sphere(642);
        let first = sample_spherical_field(&sphere, PLATE_RESISTANCE_RECIPE, 71).unwrap();
        let repeated = sample_spherical_field(&sphere, PLATE_RESISTANCE_RECIPE, 71).unwrap();
        let changed = sample_spherical_field(&sphere, PLATE_RESISTANCE_RECIPE, 72).unwrap();

        assert_eq!(first.values(), repeated.values());
        assert_ne!(first.values(), changed.values());
        assert!(area_weighted_mean(&sphere, &first).abs() <= 2.0 / f64::from(i16::MAX));
        assert_cut_and_pole_neighbor_jumps_are_bounded(&sphere, &first);
        assert_eq!(first.len(), sphere.cells().len());
        assert_eq!(first.get(CellId::from_raw(0)), Some(first.values()[0]));
    }

    #[test]
    fn unresolvable_detail_band_is_omitted_without_changing_macro_values() {
        let coarse = test_sphere(162);
        let macro_only = sample_spherical_field(&coarse, MACRO_ONLY_RECIPE, 91).unwrap();
        let with_unresolvable_detail =
            sample_spherical_field(&coarse, MACRO_PLUS_TINY_DETAIL_RECIPE, 91).unwrap();

        assert_eq!(macro_only.values(), with_unresolvable_detail.values());
    }
}
