use std::collections::BTreeMap;

use super::conservative_remap::ConservativeRemapError;
use crate::world::spatial::ConservativeSurfaceMap;
use crate::world::CellId;

const EXTENSIVE_CONSERVATION_LIMIT: f64 = 1.0e-6;
const CATEGORY_HALF_TOLERANCE: f64 = 64.0 * f64::EPSILON;
const CANCELLATION_POLL_STRIDE: usize = 256;

type CancellationCheck<'a> = Option<&'a dyn Fn() -> bool>;

/// A conservative extensive-field result and its measured global budget closure.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensiveRemap {
    values: Vec<f64>,
    source_total: f64,
    target_total: f64,
    absolute_error: f64,
    relative_error: f64,
}

impl ExtensiveRemap {
    /// Returns remapped per-target-cell extensive amounts.
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// Returns the compensated global source total.
    pub const fn source_total(&self) -> f64 {
        self.source_total
    }

    /// Returns the compensated global target total.
    pub const fn target_total(&self) -> f64 {
        self.target_total
    }

    /// Returns the absolute global conservation error.
    pub const fn absolute_error(&self) -> f64 {
        self.absolute_error
    }

    /// Returns the scale-aware global conservation error.
    pub const fn relative_error(&self) -> f64 {
        self.relative_error
    }
}

/// A stable categorical majority remap and its target-area ambiguity coverage.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoricalRemap {
    values: Vec<u16>,
    ambiguous_target_area_fraction: f64,
}

impl CategoricalRemap {
    /// Returns the selected category for every target cell.
    pub fn values(&self) -> &[u16] {
        &self.values
    }

    /// Returns target area where no category owns more than half the row overlap.
    pub const fn ambiguous_target_area_fraction(&self) -> f64 {
        self.ambiguous_target_area_fraction
    }
}

/// Remaps a finite intensive `f64` scalar with bounded area-weighted interpolation.
pub fn remap_intensive_f64(
    map: &ConservativeSurfaceMap,
    source: &[f64],
) -> Result<Vec<f64>, ConservativeRemapError> {
    remap_intensive_f64_impl(map, source, None)
}

pub fn remap_intensive_f64_cancellable(
    map: &ConservativeSurfaceMap,
    source: &[f64],
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<f64>, ConservativeRemapError> {
    remap_intensive_f64_impl(map, source, Some(cancelled))
}

fn remap_intensive_f64_impl(
    map: &ConservativeSurfaceMap,
    source: &[f64],
    cancellation: CancellationCheck<'_>,
) -> Result<Vec<f64>, ConservativeRemapError> {
    let constant = validate_scalar_input(map, "intensive_f64", source, cancellation)?;
    if constant {
        return Ok(vec![source[0]; map.target_ref().cell_count() as usize]);
    }

    let mut target = Vec::with_capacity(map.target_ref().cell_count() as usize);
    for target_index in 0..map.target_ref().cell_count() as usize {
        poll_cancelled(target_index, cancellation)?;
        let row = map
            .target_row(CellId::from_raw(target_index as u32))
            .expect("validated conservative maps contain every target row");
        let mut weighted_sum = FieldSum::default();
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        for weight in row {
            let value = source[weight.source_cell().raw() as usize];
            minimum = minimum.min(value);
            maximum = maximum.max(value);
            let contribution = value * weight.area_m2();
            if !contribution.is_finite() {
                return Err(ConservativeRemapError::NonFiniteFieldAccumulation {
                    field: "intensive_f64",
                    target_cell: CellId::from_raw(target_index as u32),
                });
            }
            weighted_sum.add(contribution, "intensive_f64", target_index)?;
        }
        let value = weighted_sum.total("intensive_f64", target_index)?
            / map.target_cell_areas_m2()[target_index];
        if !value.is_finite() {
            return Err(ConservativeRemapError::NonFiniteFieldAccumulation {
                field: "intensive_f64",
                target_cell: CellId::from_raw(target_index as u32),
            });
        }
        target.push(value.clamp(minimum, maximum));
    }
    Ok(target)
}

/// Remaps an intensive `f32` scalar and preserves a bitwise constant exactly.
pub fn remap_intensive_f32(
    map: &ConservativeSurfaceMap,
    source: &[f32],
) -> Result<Vec<f32>, ConservativeRemapError> {
    remap_intensive_f32_impl(map, source, None)
}

pub fn remap_intensive_f32_cancellable(
    map: &ConservativeSurfaceMap,
    source: &[f32],
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<f32>, ConservativeRemapError> {
    remap_intensive_f32_impl(map, source, Some(cancelled))
}

fn remap_intensive_f32_impl(
    map: &ConservativeSurfaceMap,
    source: &[f32],
    cancellation: CancellationCheck<'_>,
) -> Result<Vec<f32>, ConservativeRemapError> {
    validate_length(map, "intensive_f32", source.len())?;
    check_field_cancelled(cancellation)?;
    let mut constant = true;
    for (index, &value) in source.iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        if !value.is_finite() {
            return Err(ConservativeRemapError::NonFiniteFieldValue {
                field: "intensive_f32",
                index,
                component: None,
                found: f64::from(value),
            });
        }
        constant &= value.to_bits() == source[0].to_bits();
    }
    if constant {
        return Ok(vec![source[0]; map.target_ref().cell_count() as usize]);
    }
    let mut widened = Vec::with_capacity(source.len());
    for (index, &value) in source.iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        widened.push(f64::from(value));
    }
    let remapped = remap_intensive_f64_impl(map, &widened, cancellation)?;
    let mut quantized_values = Vec::with_capacity(remapped.len());
    for (index, value) in remapped.into_iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        let quantized = value as f32;
        if !quantized.is_finite() {
            return Err(ConservativeRemapError::FieldQuantizationOverflow {
                field: "intensive_f32",
                target_cell: CellId::from_raw(index as u32),
                found: value,
            });
        }
        quantized_values.push(quantized);
    }
    Ok(quantized_values)
}

/// Conservatively remaps finite extensive per-cell amounts.
pub fn remap_extensive_f64(
    map: &ConservativeSurfaceMap,
    source: &[f64],
) -> Result<ExtensiveRemap, ConservativeRemapError> {
    remap_extensive_f64_impl(map, source, None)
}

pub fn remap_extensive_f64_cancellable(
    map: &ConservativeSurfaceMap,
    source: &[f64],
    cancelled: &dyn Fn() -> bool,
) -> Result<ExtensiveRemap, ConservativeRemapError> {
    remap_extensive_f64_impl(map, source, Some(cancelled))
}

fn remap_extensive_f64_impl(
    map: &ConservativeSurfaceMap,
    source: &[f64],
    cancellation: CancellationCheck<'_>,
) -> Result<ExtensiveRemap, ConservativeRemapError> {
    validate_scalar_input(map, "extensive_f64", source, cancellation)?;
    let mut target = Vec::with_capacity(map.target_ref().cell_count() as usize);
    for target_index in 0..map.target_ref().cell_count() as usize {
        poll_cancelled(target_index, cancellation)?;
        let row = map
            .target_row(CellId::from_raw(target_index as u32))
            .expect("validated conservative maps contain every target row");
        let mut sum = FieldSum::default();
        for weight in row {
            let source_index = weight.source_cell().raw() as usize;
            let contribution =
                source[source_index] * weight.area_m2() / map.source_cell_areas_m2()[source_index];
            if !contribution.is_finite() {
                return Err(ConservativeRemapError::NonFiniteFieldAccumulation {
                    field: "extensive_f64",
                    target_cell: CellId::from_raw(target_index as u32),
                });
            }
            sum.add(contribution, "extensive_f64", target_index)?;
        }
        target.push(sum.total("extensive_f64", target_index)?);
    }

    let source_total = total(source, "extensive_f64", cancellation)?;
    let target_total = total(&target, "extensive_f64", cancellation)?;
    let absolute_scale = total_absolute(source, "extensive_f64", cancellation)?;
    let absolute_error = (source_total - target_total).abs();
    let relative_error = if absolute_scale == 0.0 {
        absolute_error
    } else {
        absolute_error / absolute_scale
    };
    if !relative_error.is_finite() || relative_error > EXTENSIVE_CONSERVATION_LIMIT {
        return Err(ConservativeRemapError::ExtensiveConservationExceeded {
            source_total,
            target_total,
            absolute_scale,
            relative_error,
            max: EXTENSIVE_CONSERVATION_LIMIT,
        });
    }
    Ok(ExtensiveRemap {
        values: target,
        source_total,
        target_total,
        absolute_error,
        relative_error,
    })
}

/// Transports and area-weights source east/north tangent components.
pub fn remap_tangent_components_f64(
    map: &ConservativeSurfaceMap,
    source_east_north: &[[f64; 2]],
) -> Result<Vec<[f64; 2]>, ConservativeRemapError> {
    remap_tangent_components_f64_impl(map, source_east_north, None)
}

pub fn remap_tangent_components_f64_cancellable(
    map: &ConservativeSurfaceMap,
    source_east_north: &[[f64; 2]],
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<[f64; 2]>, ConservativeRemapError> {
    remap_tangent_components_f64_impl(map, source_east_north, Some(cancelled))
}

fn remap_tangent_components_f64_impl(
    map: &ConservativeSurfaceMap,
    source_east_north: &[[f64; 2]],
    cancellation: CancellationCheck<'_>,
) -> Result<Vec<[f64; 2]>, ConservativeRemapError> {
    validate_length(map, "tangent_f64", source_east_north.len())?;
    check_field_cancelled(cancellation)?;
    for (index, components) in source_east_north.iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        for (component, &value) in components.iter().enumerate() {
            if !value.is_finite() {
                return Err(ConservativeRemapError::NonFiniteFieldValue {
                    field: "tangent_f64",
                    index,
                    component: Some(component),
                    found: value,
                });
            }
        }
    }

    let mut target = Vec::with_capacity(map.target_ref().cell_count() as usize);
    for target_index in 0..map.target_ref().cell_count() as usize {
        poll_cancelled(target_index, cancellation)?;
        let row = map
            .target_row(CellId::from_raw(target_index as u32))
            .expect("validated conservative maps contain every target row");
        let mut sums = [FieldSum::default(); 2];
        for weight in row {
            let transformed = weight
                .tangent_transform()
                .apply(source_east_north[weight.source_cell().raw() as usize]);
            for component in 0..2 {
                let contribution = transformed[component] * weight.area_m2();
                if !contribution.is_finite() {
                    return Err(ConservativeRemapError::NonFiniteFieldAccumulation {
                        field: "tangent_f64",
                        target_cell: CellId::from_raw(target_index as u32),
                    });
                }
                sums[component].add(contribution, "tangent_f64", target_index)?;
            }
        }
        let inverse_area = 1.0 / map.target_cell_areas_m2()[target_index];
        let value = [
            sums[0].total("tangent_f64", target_index)? * inverse_area,
            sums[1].total("tangent_f64", target_index)? * inverse_area,
        ];
        if value.iter().any(|component| !component.is_finite()) {
            return Err(ConservativeRemapError::NonFiniteFieldAccumulation {
                field: "tangent_f64",
                target_cell: CellId::from_raw(target_index as u32),
            });
        }
        target.push(value);
    }
    Ok(target)
}

/// Selects stable overlap-majority categories and reports ambiguous target area.
pub fn remap_categories_u16(
    map: &ConservativeSurfaceMap,
    source: &[u16],
) -> Result<CategoricalRemap, ConservativeRemapError> {
    validate_length(map, "categories_u16", source.len())?;
    let mut target = Vec::with_capacity(map.target_ref().cell_count() as usize);
    let mut ambiguous_area = FieldSum::default();
    for target_index in 0..map.target_ref().cell_count() as usize {
        let row = map
            .target_row(CellId::from_raw(target_index as u32))
            .expect("validated conservative maps contain every target row");
        let mut category_areas = BTreeMap::<u16, FieldSum>::new();
        let mut row_area = FieldSum::default();
        for weight in row {
            let category = source[weight.source_cell().raw() as usize];
            category_areas.entry(category).or_default().add(
                weight.area_m2(),
                "categories_u16",
                target_index,
            )?;
            row_area.add(weight.area_m2(), "categories_u16", target_index)?;
        }
        let row_area = row_area.total("categories_u16", target_index)?;
        let mut selected = None;
        let mut selected_area = f64::NEG_INFINITY;
        for (category, sum) in category_areas {
            let area = sum.total("categories_u16", target_index)?;
            if area > selected_area {
                selected = Some(category);
                selected_area = area;
            }
        }
        target.push(selected.expect("validated target rows contain overlaps"));
        if selected_area * 2.0 <= row_area * (1.0 + CATEGORY_HALF_TOLERANCE) {
            ambiguous_area.add(
                map.target_cell_areas_m2()[target_index],
                "categories_u16",
                target_index,
            )?;
        }
    }
    let total_target_area = total(map.target_cell_areas_m2(), "categories_u16", None)?;
    let ambiguous_target_area_fraction =
        ambiguous_area.total("categories_u16", 0)? / total_target_area;
    Ok(CategoricalRemap {
        values: target,
        ambiguous_target_area_fraction,
    })
}

fn validate_scalar_input(
    map: &ConservativeSurfaceMap,
    field: &'static str,
    source: &[f64],
    cancellation: CancellationCheck<'_>,
) -> Result<bool, ConservativeRemapError> {
    validate_length(map, field, source.len())?;
    check_field_cancelled(cancellation)?;
    let mut constant = true;
    for (index, &found) in source.iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        if !found.is_finite() {
            return Err(ConservativeRemapError::NonFiniteFieldValue {
                field,
                index,
                component: None,
                found,
            });
        }
        constant &= found.to_bits() == source[0].to_bits();
    }
    Ok(constant)
}

fn validate_length(
    map: &ConservativeSurfaceMap,
    field: &'static str,
    found: usize,
) -> Result<(), ConservativeRemapError> {
    let expected = map.source_ref().cell_count() as usize;
    if found != expected {
        return Err(ConservativeRemapError::FieldLengthMismatch {
            field,
            found,
            expected,
        });
    }
    Ok(())
}

fn total(
    values: &[f64],
    field: &'static str,
    cancellation: CancellationCheck<'_>,
) -> Result<f64, ConservativeRemapError> {
    let mut sum = FieldSum::default();
    for (index, &value) in values.iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        sum.add(value, field, index)?;
    }
    sum.total(field, values.len().saturating_sub(1))
}

fn total_absolute(
    values: &[f64],
    field: &'static str,
    cancellation: CancellationCheck<'_>,
) -> Result<f64, ConservativeRemapError> {
    let mut sum = FieldSum::default();
    for (index, &value) in values.iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        sum.add(value.abs(), field, index)?;
    }
    sum.total(field, values.len().saturating_sub(1))
}

fn poll_cancelled(
    index: usize,
    cancellation: CancellationCheck<'_>,
) -> Result<(), ConservativeRemapError> {
    if index % CANCELLATION_POLL_STRIDE == 0 {
        check_field_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_field_cancelled(
    cancellation: CancellationCheck<'_>,
) -> Result<(), ConservativeRemapError> {
    if cancellation.is_some_and(|cancelled| cancelled()) {
        Err(ConservativeRemapError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct FieldSum {
    sum: f64,
    correction: f64,
}

impl FieldSum {
    fn add(
        &mut self,
        value: f64,
        field: &'static str,
        target_index: usize,
    ) -> Result<(), ConservativeRemapError> {
        let next = self.sum + value;
        if !next.is_finite() {
            return Err(ConservativeRemapError::NonFiniteFieldAccumulation {
                field,
                target_cell: CellId::from_raw(target_index as u32),
            });
        }
        let correction = if self.sum.abs() >= value.abs() {
            (self.sum - next) + value
        } else {
            (value - next) + self.sum
        };
        self.sum = next;
        self.correction += correction;
        if !self.correction.is_finite() {
            return Err(ConservativeRemapError::NonFiniteFieldAccumulation {
                field,
                target_cell: CellId::from_raw(target_index as u32),
            });
        }
        Ok(())
    }

    fn total(
        self,
        field: &'static str,
        target_index: usize,
    ) -> Result<f64, ConservativeRemapError> {
        let total = self.sum + self.correction;
        total.is_finite().then_some(total).ok_or(
            ConservativeRemapError::NonFiniteFieldAccumulation {
                field,
                target_cell: CellId::from_raw(target_index as u32),
            },
        )
    }
}
