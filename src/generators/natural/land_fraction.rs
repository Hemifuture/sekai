use thiserror::Error;

use crate::world::natural::LandOceanKind;

/// One deterministic area-weighted sea-level selection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LandFractionSelection {
    pub(super) sea_level_m: f32,
    pub(super) actual_land_fraction: f64,
    pub(super) target_land_fraction: f64,
}

/// Invalid inputs that prevent an area-weighted land selection.
#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum LandFractionSelectionError {
    #[error("cannot select a sea level from an empty surface")]
    Empty,
    #[error("area/elevation cardinality mismatch: {areas} != {elevations}")]
    CardinalityMismatch { areas: usize, elevations: usize },
    #[error("cell area at index {index} must be finite and positive, found {found}")]
    InvalidArea { index: usize, found: f64 },
    #[error("total surface area must be finite and positive, found {found}")]
    InvalidTotalArea { found: f64 },
    #[error("elevation at index {index} must be finite, found {found}")]
    InvalidElevation { index: usize, found: f32 },
    #[error("target land fraction must be finite and within 0..=1, found {found}")]
    InvalidTarget { found: f64 },
    #[error("no finite sea level can represent the selected land prefix")]
    NoFiniteSeaLevel,
}

/// Selects the closest representable land fraction without splitting equal-height plateaus.
pub(super) fn select_area_weighted_sea_level(
    cell_areas: &[f64],
    elevations_m: &[f32],
    target_land_fraction: f64,
) -> Result<LandFractionSelection, LandFractionSelectionError> {
    if cell_areas.is_empty() && elevations_m.is_empty() {
        return Err(LandFractionSelectionError::Empty);
    }
    if cell_areas.len() != elevations_m.len() {
        return Err(LandFractionSelectionError::CardinalityMismatch {
            areas: cell_areas.len(),
            elevations: elevations_m.len(),
        });
    }
    if !target_land_fraction.is_finite() || !(0.0..=1.0).contains(&target_land_fraction) {
        return Err(LandFractionSelectionError::InvalidTarget {
            found: target_land_fraction,
        });
    }

    let mut total_area = 0.0;
    let mut ranked = Vec::with_capacity(cell_areas.len());
    for (index, (&area, &elevation)) in cell_areas.iter().zip(elevations_m).enumerate() {
        if !area.is_finite() || area <= 0.0 {
            return Err(LandFractionSelectionError::InvalidArea { index, found: area });
        }
        if !elevation.is_finite() {
            return Err(LandFractionSelectionError::InvalidElevation {
                index,
                found: elevation,
            });
        }
        total_area += area;
        ranked.push((
            LandOceanKind::quantized_centimeters(elevation),
            index,
            elevation,
            area,
        ));
    }
    if !total_area.is_finite() || total_area <= 0.0 {
        return Err(LandFractionSelectionError::InvalidTotalArea { found: total_area });
    }

    ranked.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));

    let mut best_level = None;
    let mut best_error = target_land_fraction;
    let mut selected_area = 0.0;
    let mut cursor = 0;
    while cursor < ranked.len() {
        let plateau = ranked[cursor].0;
        let plateau_elevation = ranked[cursor].2;
        while cursor < ranked.len() && ranked[cursor].0 == plateau {
            selected_area += ranked[cursor].3;
            cursor += 1;
        }
        let fraction = selected_area / total_area;
        let error = (fraction - target_land_fraction).abs();
        if error < best_error {
            best_error = error;
            best_level = Some(plateau_elevation);
        }
    }

    let sea_level_m = if let Some(level) = best_level {
        level
    } else {
        sea_level_above_plateau(ranked[0].0).ok_or(LandFractionSelectionError::NoFiniteSeaLevel)?
    };
    let actual_land_area = cell_areas
        .iter()
        .zip(elevations_m)
        .filter_map(|(&area, &elevation)| {
            (LandOceanKind::classify(elevation, sea_level_m) == LandOceanKind::Land).then_some(area)
        })
        .sum::<f64>();
    Ok(LandFractionSelection {
        sea_level_m,
        actual_land_fraction: actual_land_area / total_area,
        target_land_fraction,
    })
}

fn sea_level_above_plateau(plateau: i64) -> Option<f32> {
    if plateau == i64::MAX {
        return None;
    }
    let mut candidate = ((plateau as f64 + 1.0) / 100.0) as f32;
    while candidate.is_finite() && LandOceanKind::quantized_centimeters(candidate) <= plateau {
        candidate = next_up(candidate)?;
    }
    candidate.is_finite().then_some(candidate)
}

fn next_up(value: f32) -> Option<f32> {
    if value == f32::INFINITY {
        return None;
    }
    if value == 0.0 {
        return Some(f32::from_bits(1));
    }
    let bits = value.to_bits();
    Some(f32::from_bits(if value > 0.0 {
        bits + 1
    } else {
        bits - 1
    }))
}

#[cfg(test)]
mod tests {
    use super::{select_area_weighted_sea_level, LandFractionSelectionError};

    #[test]
    fn weighted_selection_uses_surface_area_instead_of_cell_count() {
        let elevations = [100.0_f32, 90.0, 80.0, 70.0];
        let original_bits = elevations.map(f32::to_bits);

        let selection =
            select_area_weighted_sea_level(&[0.60, 0.20, 0.10, 0.10], &elevations, 0.60).unwrap();

        assert_eq!(selection.sea_level_m, 100.0);
        assert!((selection.actual_land_fraction - 0.60).abs() <= f64::EPSILON);
        assert_eq!(elevations.map(f32::to_bits), original_bits);
    }

    #[test]
    fn weighted_selection_never_splits_an_equal_elevation_plateau() {
        let selection =
            select_area_weighted_sea_level(&[0.40, 0.30, 0.30], &[100.001, 100.004, 0.0], 0.40)
                .unwrap();

        assert!(selection.sea_level_m >= 100.001);
        assert!((selection.actual_land_fraction - 0.70).abs() <= f64::EPSILON);
        assert!((selection.target_land_fraction - 0.40).abs() <= f64::EPSILON);
    }

    #[test]
    fn weighted_selection_is_monotone_across_authored_targets() {
        let areas = [0.28, 0.24, 0.20, 0.16, 0.12];
        let elevations = [4.0, 3.0, 2.0, 1.0, 0.0];
        let low = select_area_weighted_sea_level(&areas, &elevations, 0.25).unwrap();
        let middle = select_area_weighted_sea_level(&areas, &elevations, 0.50).unwrap();
        let high = select_area_weighted_sea_level(&areas, &elevations, 0.70).unwrap();

        assert!(low.actual_land_fraction <= middle.actual_land_fraction);
        assert!(middle.actual_land_fraction <= high.actual_land_fraction);
        assert!(low.sea_level_m >= middle.sea_level_m);
        assert!(middle.sea_level_m >= high.sea_level_m);
    }

    #[test]
    fn weighted_selection_rejects_malformed_inputs() {
        assert_eq!(
            select_area_weighted_sea_level(&[], &[], 0.5),
            Err(LandFractionSelectionError::Empty)
        );
        assert_eq!(
            select_area_weighted_sea_level(&[1.0], &[1.0, 2.0], 0.5),
            Err(LandFractionSelectionError::CardinalityMismatch {
                areas: 1,
                elevations: 2,
            })
        );
        for areas in [[f64::NAN], [-1.0], [0.0]] {
            assert!(matches!(
                select_area_weighted_sea_level(&areas, &[1.0], 0.5),
                Err(LandFractionSelectionError::InvalidArea { .. })
                    | Err(LandFractionSelectionError::InvalidTotalArea { .. })
            ));
        }
        assert!(matches!(
            select_area_weighted_sea_level(&[1.0], &[f32::NAN], 0.5),
            Err(LandFractionSelectionError::InvalidElevation { .. })
        ));
        for target in [f64::NAN, -0.1, 1.1] {
            assert!(matches!(
                select_area_weighted_sea_level(&[1.0], &[1.0], target),
                Err(LandFractionSelectionError::InvalidTarget { .. })
            ));
        }
    }
}
