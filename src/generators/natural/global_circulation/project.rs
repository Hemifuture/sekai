use thiserror::Error;

use crate::generators::spatial::{
    remap_extensive_f64, remap_intensive_f32, remap_tangent_components_f64, ConservativeRemapError,
};
use crate::world::natural::{ClimateWorkDomainSnapshot, CLIMATE_MONTH_COUNT};
use crate::world::spatial::{canonical_east_north_basis, SphericalSurfaceSnapshot};

/// A projected monthly scalar plus any applicable conservative budget evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedMonthlyScalar {
    values: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    max_relative_conservation_error: f64,
}

impl ProjectedMonthlyScalar {
    pub fn values(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.values
    }

    pub const fn max_relative_conservation_error(&self) -> f64 {
        self.max_relative_conservation_error
    }
}

/// Projects a climate-grid intensive climatology by bounded overlap averaging.
pub fn project_monthly_intensive_scalar(
    domain: &ClimateWorkDomainSnapshot,
    climate_values: &[[f32; CLIMATE_MONTH_COUNT]],
) -> Result<ProjectedMonthlyScalar, ClimateProjectionError> {
    validate_scalar_input(domain, "monthly_intensive_scalar", climate_values, false)?;
    let target_count = domain.source_ref().cell_count() as usize;
    let mut values = vec![[0.0_f32; CLIMATE_MONTH_COUNT]; target_count];
    for month in 0..CLIMATE_MONTH_COUNT {
        let source = climate_values
            .iter()
            .map(|months| months[month])
            .collect::<Vec<_>>();
        let projected = remap_intensive_f32(domain.climate_to_source(), &source)?;
        for (target, value) in values.iter_mut().zip(projected) {
            target[month] = value;
        }
    }
    Ok(ProjectedMonthlyScalar {
        values,
        max_relative_conservation_error: 0.0,
    })
}

/// Projects a nonnegative per-area rate by remapping per-cell extensive amounts
/// and dividing by authoritative target area.
pub fn project_monthly_extensive_rate(
    domain: &ClimateWorkDomainSnapshot,
    climate_rate: &[[f32; CLIMATE_MONTH_COUNT]],
) -> Result<ProjectedMonthlyScalar, ClimateProjectionError> {
    validate_scalar_input(domain, "monthly_extensive_rate", climate_rate, true)?;
    let map = domain.climate_to_source();
    let target_count = domain.source_ref().cell_count() as usize;
    let mut values = vec![[0.0_f32; CLIMATE_MONTH_COUNT]; target_count];
    let mut max_relative_error = 0.0_f64;
    for month in 0..CLIMATE_MONTH_COUNT {
        let source_amount = climate_rate
            .iter()
            .zip(map.source_cell_areas_m2())
            .map(|(months, area)| f64::from(months[month]) * area)
            .collect::<Vec<_>>();
        let projected = remap_extensive_f64(map, &source_amount)?;
        max_relative_error = max_relative_error.max(projected.relative_error());
        for (target_index, target) in values.iter_mut().enumerate() {
            let rate = projected.values()[target_index] / map.target_cell_areas_m2()[target_index];
            let quantized = rate as f32;
            if !quantized.is_finite() || quantized < 0.0 {
                return Err(ClimateProjectionError::QuantizationOverflow {
                    field: "monthly_extensive_rate",
                    cell: target_index,
                    month,
                    found: rate,
                });
            }
            target[month] = quantized;
        }
    }
    Ok(ProjectedMonthlyScalar {
        values,
        max_relative_conservation_error: max_relative_error,
    })
}

/// Transports global tangent vectors through the map's stored basis rotations
/// and republishes global tangent vectors on the authoritative surface.
pub fn project_monthly_tangent_vectors(
    domain: &ClimateWorkDomainSnapshot,
    authoritative_surface: &SphericalSurfaceSnapshot,
    climate_vectors: &[[[f32; 3]; CLIMATE_MONTH_COUNT]],
) -> Result<Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>, ClimateProjectionError> {
    domain
        .validate_against(authoritative_surface)
        .map_err(|error| ClimateProjectionError::InvalidDomain {
            reason: error.to_string(),
        })?;
    let expected = domain.climate_surface().cells().len();
    if climate_vectors.len() != expected {
        return Err(ClimateProjectionError::LengthMismatch {
            field: "monthly_tangent_vectors",
            found: climate_vectors.len(),
            expected,
        });
    }
    for (cell, months) in climate_vectors.iter().enumerate() {
        for (month, vector) in months.iter().enumerate() {
            for (component, value) in vector.iter().copied().enumerate() {
                if !value.is_finite() {
                    return Err(ClimateProjectionError::NonFiniteVector {
                        cell,
                        month,
                        component,
                    });
                }
            }
        }
    }

    let target_count = domain.source_ref().cell_count() as usize;
    let mut output = vec![[[0.0_f32; 3]; CLIMATE_MONTH_COUNT]; target_count];
    for month in 0..CLIMATE_MONTH_COUNT {
        let source_local = domain
            .climate_surface()
            .cells()
            .iter()
            .zip(climate_vectors)
            .map(|(cell, months)| {
                let (east, north) = canonical_east_north_basis(cell.centroid);
                let vector = months[month].map(f64::from);
                [dot(vector, east), dot(vector, north)]
            })
            .collect::<Vec<_>>();
        let target_local = remap_tangent_components_f64(domain.climate_to_source(), &source_local)?;
        for (target_index, (cell, local)) in authoritative_surface
            .cells()
            .iter()
            .zip(target_local)
            .enumerate()
        {
            let (east, north) = canonical_east_north_basis(cell.centroid);
            let global = [
                east[0] * local[0] + north[0] * local[1],
                east[1] * local[0] + north[1] * local[1],
                east[2] * local[0] + north[2] * local[1],
            ];
            for (component, value) in global.into_iter().enumerate() {
                let quantized = value as f32;
                if !quantized.is_finite() {
                    return Err(ClimateProjectionError::QuantizationOverflow {
                        field: "monthly_tangent_vectors",
                        cell: target_index,
                        month,
                        found: value,
                    });
                }
                output[target_index][month][component] = quantized;
            }
        }
    }
    Ok(output)
}

fn validate_scalar_input(
    domain: &ClimateWorkDomainSnapshot,
    field: &'static str,
    values: &[[f32; CLIMATE_MONTH_COUNT]],
    nonnegative: bool,
) -> Result<(), ClimateProjectionError> {
    let expected = domain.climate_surface().cells().len();
    if values.len() != expected {
        return Err(ClimateProjectionError::LengthMismatch {
            field,
            found: values.len(),
            expected,
        });
    }
    for (cell, months) in values.iter().enumerate() {
        for (month, value) in months.iter().copied().enumerate() {
            if !value.is_finite() || (nonnegative && value < 0.0) {
                return Err(ClimateProjectionError::InvalidScalar {
                    field,
                    cell,
                    month,
                    found: value,
                });
            }
        }
    }
    Ok(())
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateProjectionError {
    #[error("invalid climate projection domain: {reason}")]
    InvalidDomain { reason: String },
    #[error("{field} has {found} climate cells, expected {expected}")]
    LengthMismatch {
        field: &'static str,
        found: usize,
        expected: usize,
    },
    #[error("{field}[{cell}][{month}] is invalid: {found}")]
    InvalidScalar {
        field: &'static str,
        cell: usize,
        month: usize,
        found: f32,
    },
    #[error("monthly tangent vector [{cell}][{month}][{component}] is non-finite")]
    NonFiniteVector {
        cell: usize,
        month: usize,
        component: usize,
    },
    #[error("{field}[{cell}][{month}] cannot be quantized from {found}")]
    QuantizationOverflow {
        field: &'static str,
        cell: usize,
        month: usize,
        found: f64,
    },
    #[error(transparent)]
    Remap(#[from] ConservativeRemapError),
}
