use thiserror::Error;

use crate::engine::BuildCancellation;
use crate::generators::spatial::{
    remap_extensive_f64, remap_extensive_f64_cancellable, remap_intensive_f32,
    remap_intensive_f32_cancellable, remap_tangent_components_f64,
    remap_tangent_components_f64_cancellable, ConservativeRemapError,
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

    pub(crate) fn into_values(self) -> Vec<[f32; CLIMATE_MONTH_COUNT]> {
        self.values
    }

    pub const fn max_relative_conservation_error(&self) -> f64 {
        self.max_relative_conservation_error
    }
}

/// Projects one time-invariant intensive scalar by bounded overlap averaging.
pub(crate) fn project_intensive_scalar_cancellable(
    domain: &ClimateWorkDomainSnapshot,
    climate_values: &[f32],
    cancellation: &BuildCancellation,
) -> Result<Vec<f32>, ClimateProjectionError> {
    check_projection_cancelled(Some(cancellation))?;
    let expected = domain.climate_surface().cells().len();
    if climate_values.len() != expected {
        return Err(ClimateProjectionError::LengthMismatch {
            field: "intensive_scalar",
            found: climate_values.len(),
            expected,
        });
    }
    for (cell, value) in climate_values.iter().copied().enumerate() {
        poll_projection_cancelled(cell, Some(cancellation))?;
        if !value.is_finite() {
            return Err(ClimateProjectionError::InvalidScalar {
                field: "intensive_scalar",
                cell,
                month: 0,
                found: value,
            });
        }
    }
    remap_intensive_f32_cancellable(domain.climate_to_source(), climate_values, &|| {
        cancellation.is_cancelled()
    })
    .map_err(map_remap_error)
}

/// Projects a climate-grid intensive climatology by bounded overlap averaging.
pub fn project_monthly_intensive_scalar(
    domain: &ClimateWorkDomainSnapshot,
    authoritative_surface: &SphericalSurfaceSnapshot,
    climate_values: &[[f32; CLIMATE_MONTH_COUNT]],
) -> Result<ProjectedMonthlyScalar, ClimateProjectionError> {
    domain
        .validate_against(authoritative_surface)
        .map_err(|error| ClimateProjectionError::InvalidDomain {
            reason: error.to_string(),
        })?;
    project_monthly_intensive_scalar_impl(domain, climate_values, None)
}

pub(crate) fn project_monthly_intensive_scalar_cancellable(
    domain: &ClimateWorkDomainSnapshot,
    climate_values: &[[f32; CLIMATE_MONTH_COUNT]],
    cancellation: &BuildCancellation,
) -> Result<ProjectedMonthlyScalar, ClimateProjectionError> {
    project_monthly_intensive_scalar_impl(domain, climate_values, Some(cancellation))
}

fn project_monthly_intensive_scalar_impl(
    domain: &ClimateWorkDomainSnapshot,
    climate_values: &[[f32; CLIMATE_MONTH_COUNT]],
    cancellation: Option<&BuildCancellation>,
) -> Result<ProjectedMonthlyScalar, ClimateProjectionError> {
    check_projection_cancelled(cancellation)?;
    validate_scalar_input(
        domain,
        "monthly_intensive_scalar",
        climate_values,
        false,
        cancellation,
    )?;
    let target_count = domain.source_ref().cell_count() as usize;
    let mut values = vec![[0.0_f32; CLIMATE_MONTH_COUNT]; target_count];
    for month in 0..CLIMATE_MONTH_COUNT {
        check_projection_cancelled(cancellation)?;
        let mut source = Vec::with_capacity(climate_values.len());
        for (cell, months) in climate_values.iter().enumerate() {
            poll_projection_cancelled(cell, cancellation)?;
            source.push(months[month]);
        }
        let projected = match cancellation {
            Some(cancellation) => {
                remap_intensive_f32_cancellable(domain.climate_to_source(), &source, &|| {
                    cancellation.is_cancelled()
                })
            }
            None => remap_intensive_f32(domain.climate_to_source(), &source),
        }
        .map_err(map_remap_error)?;
        for (target_index, (target, value)) in values.iter_mut().zip(projected).enumerate() {
            poll_projection_cancelled(target_index, cancellation)?;
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
    authoritative_surface: &SphericalSurfaceSnapshot,
    climate_rate: &[[f32; CLIMATE_MONTH_COUNT]],
) -> Result<ProjectedMonthlyScalar, ClimateProjectionError> {
    domain
        .validate_against(authoritative_surface)
        .map_err(|error| ClimateProjectionError::InvalidDomain {
            reason: error.to_string(),
        })?;
    project_monthly_extensive_rate_impl(domain, climate_rate, None)
}

pub(crate) fn project_monthly_extensive_rate_cancellable(
    domain: &ClimateWorkDomainSnapshot,
    climate_rate: &[[f32; CLIMATE_MONTH_COUNT]],
    cancellation: &BuildCancellation,
) -> Result<ProjectedMonthlyScalar, ClimateProjectionError> {
    project_monthly_extensive_rate_impl(domain, climate_rate, Some(cancellation))
}

fn project_monthly_extensive_rate_impl(
    domain: &ClimateWorkDomainSnapshot,
    climate_rate: &[[f32; CLIMATE_MONTH_COUNT]],
    cancellation: Option<&BuildCancellation>,
) -> Result<ProjectedMonthlyScalar, ClimateProjectionError> {
    check_projection_cancelled(cancellation)?;
    validate_scalar_input(
        domain,
        "monthly_extensive_rate",
        climate_rate,
        true,
        cancellation,
    )?;
    let map = domain.climate_to_source();
    let target_count = domain.source_ref().cell_count() as usize;
    let mut values = vec![[0.0_f32; CLIMATE_MONTH_COUNT]; target_count];
    let mut max_relative_error = 0.0_f64;
    for month in 0..CLIMATE_MONTH_COUNT {
        check_projection_cancelled(cancellation)?;
        let mut source_amount = Vec::with_capacity(climate_rate.len());
        for (cell, (months, area)) in climate_rate
            .iter()
            .zip(map.source_cell_areas_m2())
            .enumerate()
        {
            poll_projection_cancelled(cell, cancellation)?;
            source_amount.push(f64::from(months[month]) * area);
        }
        let mut source_total = 0.0_f64;
        for (cell, amount) in source_amount.iter().copied().enumerate() {
            poll_projection_cancelled(cell, cancellation)?;
            source_total += amount;
        }
        let projected = match cancellation {
            Some(cancellation) => remap_extensive_f64_cancellable(map, &source_amount, &|| {
                cancellation.is_cancelled()
            }),
            None => remap_extensive_f64(map, &source_amount),
        }
        .map_err(map_remap_error)?;
        for (target_index, target) in values.iter_mut().enumerate() {
            poll_projection_cancelled(target_index, cancellation)?;
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
        let mut published_total = 0.0_f64;
        for (cell, (months, area)) in values.iter().zip(map.target_cell_areas_m2()).enumerate() {
            poll_projection_cancelled(cell, cancellation)?;
            published_total += f64::from(months[month]) * area;
        }
        let relative_error = if source_total.abs() > f64::EPSILON {
            (published_total - source_total).abs() / source_total.abs()
        } else {
            published_total.abs()
        };
        max_relative_error = max_relative_error.max(relative_error);
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
    project_monthly_tangent_vectors_impl(domain, authoritative_surface, climate_vectors, None)
}

pub(crate) fn project_monthly_tangent_vectors_cancellable(
    domain: &ClimateWorkDomainSnapshot,
    authoritative_surface: &SphericalSurfaceSnapshot,
    climate_vectors: &[[[f32; 3]; CLIMATE_MONTH_COUNT]],
    cancellation: &BuildCancellation,
) -> Result<Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>, ClimateProjectionError> {
    project_monthly_tangent_vectors_impl(
        domain,
        authoritative_surface,
        climate_vectors,
        Some(cancellation),
    )
}

fn project_monthly_tangent_vectors_impl(
    domain: &ClimateWorkDomainSnapshot,
    authoritative_surface: &SphericalSurfaceSnapshot,
    climate_vectors: &[[[f32; 3]; CLIMATE_MONTH_COUNT]],
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>, ClimateProjectionError> {
    check_projection_cancelled(cancellation)?;
    let domain_validation = if cancellation.is_some() {
        domain.validate_binding_against(authoritative_surface)
    } else {
        domain.validate_against(authoritative_surface)
    };
    domain_validation.map_err(|error| ClimateProjectionError::InvalidDomain {
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
        poll_projection_cancelled(cell, cancellation)?;
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
        check_projection_cancelled(cancellation)?;
        let mut source_local = Vec::with_capacity(climate_vectors.len());
        for (cell_index, (cell, months)) in domain
            .climate_surface()
            .cells()
            .iter()
            .zip(climate_vectors)
            .enumerate()
        {
            poll_projection_cancelled(cell_index, cancellation)?;
            let (east, north) = canonical_east_north_basis(cell.centroid);
            let vector = months[month].map(f64::from);
            source_local.push([dot(vector, east), dot(vector, north)]);
        }
        let target_local = match cancellation {
            Some(cancellation) => remap_tangent_components_f64_cancellable(
                domain.climate_to_source(),
                &source_local,
                &|| cancellation.is_cancelled(),
            ),
            None => remap_tangent_components_f64(domain.climate_to_source(), &source_local),
        }
        .map_err(map_remap_error)?;
        for (target_index, (cell, local)) in authoritative_surface
            .cells()
            .iter()
            .zip(target_local)
            .enumerate()
        {
            if target_index % 256 == 0 {
                check_projection_cancelled(cancellation)?;
            }
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
    cancellation: Option<&BuildCancellation>,
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
        poll_projection_cancelled(cell, cancellation)?;
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

fn poll_projection_cancelled(
    index: usize,
    cancellation: Option<&BuildCancellation>,
) -> Result<(), ClimateProjectionError> {
    if index % 256 == 0 {
        check_projection_cancelled(cancellation)?;
    }
    Ok(())
}

fn map_remap_error(error: ConservativeRemapError) -> ClimateProjectionError {
    if error == ConservativeRemapError::Cancelled {
        ClimateProjectionError::Cancelled
    } else {
        ClimateProjectionError::Remap(error)
    }
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn check_projection_cancelled(
    cancellation: Option<&BuildCancellation>,
) -> Result<(), ClimateProjectionError> {
    if cancellation.is_some_and(BuildCancellation::is_cancelled) {
        Err(ClimateProjectionError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateProjectionError {
    #[error("climate projection was cancelled")]
    Cancelled,
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
