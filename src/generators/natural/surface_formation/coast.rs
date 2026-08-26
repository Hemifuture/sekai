use thiserror::Error;

use crate::engine::BuildCancellation;
use crate::world::natural::{
    SedimentSourceKindField, CLIMATE_MONTH_COUNT, CRUST_DENSITY_MAX_KG_M3, CRUST_DENSITY_MIN_KG_M3,
    ELEVATION_MAX_M, ELEVATION_MIN_M, FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
    FORMATION_COASTAL_COVER_SHIELD_M, FORMATION_COASTAL_CURRENT_REFERENCE_M_S,
    FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR, FORMATION_COASTAL_WIND_REFERENCE_M_S,
    SEDIMENT_PROVENANCE_SOURCE_COUNT,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SphericalSurfaceValidationError};
use crate::world::CellId;

use super::sediment::split_mass_by_weights;

const CANCELLATION_POLL_MASK: usize = 255;

/// Borrowed fields used by one causal coastal-removal pass.
#[derive(Debug, Clone, Copy)]
pub struct CoastalInputs<'a> {
    pub elevation_m: &'a [f64],
    pub ocean_area_fraction: &'a [f64],
    pub wet_edge_fraction: &'a [f64],
    pub substrate_erodibility: &'a [f32],
    pub sediment_mass_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    pub substrate_density_kg_m3: &'a [f32],
    pub sediment_sources: &'a SedimentSourceKindField,
    pub near_surface_wind_m_s: &'a [[[f32; 3]; CLIMATE_MONTH_COUNT]],
    pub surface_ocean_current_m_s: &'a [[[f32; 3]; CLIMATE_MONTH_COUNT]],
}

/// One retained coast pass. Ocean injections are consumed by the sediment router.
#[derive(Debug, Clone, PartialEq)]
pub struct CoastalExchangeStep {
    coastal_erosion_m: Vec<f64>,
    land_exposure: Vec<f64>,
    marine_exposure: Vec<f64>,
    removed_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    ocean_injection_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    produced_mass_kg: f64,
    produced_by_source_kg: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    sediment_stock_removed_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
}

impl CoastalExchangeStep {
    pub fn coastal_erosion_m(&self) -> &[f64] {
        &self.coastal_erosion_m
    }

    pub fn land_exposure(&self) -> &[f64] {
        &self.land_exposure
    }

    pub fn marine_exposure(&self) -> &[f64] {
        &self.marine_exposure
    }

    pub fn ocean_injection_by_source_kg(&self) -> &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.ocean_injection_by_source_kg
    }

    pub fn removed_by_source_kg(&self) -> &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.removed_by_source_kg
    }

    pub const fn produced_mass_kg(&self) -> f64 {
        self.produced_mass_kg
    }

    pub const fn produced_by_source_kg(&self) -> &[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT] {
        &self.produced_by_source_kg
    }

    pub fn sediment_stock_removed_by_source_kg(
        &self,
    ) -> &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.sediment_stock_removed_by_source_kg
    }
}

/// Edge-projected P4 wind/current exposure with source-bound shelf injection.
#[derive(Debug, Clone, Copy, Default)]
pub struct CoastalExchange;

impl CoastalExchange {
    pub fn advance(
        surface: &SphericalSurfaceSnapshot,
        inputs: CoastalInputs<'_>,
        step_years: f64,
        cancellation: &BuildCancellation,
    ) -> Result<CoastalExchangeStep, CoastGenerationError> {
        check_cancelled(cancellation)?;
        surface
            .validate_cancellable(&|| cancellation.is_cancelled())
            .map_err(|error| map_surface_error(error, cancellation))?;
        Self::advance_from_validated_surface(surface, inputs, step_years, cancellation)
    }

    /// Same paired exchange for a caller that already validated the surface.
    pub(super) fn advance_from_validated_surface(
        surface: &SphericalSurfaceSnapshot,
        inputs: CoastalInputs<'_>,
        step_years: f64,
        cancellation: &BuildCancellation,
    ) -> Result<CoastalExchangeStep, CoastGenerationError> {
        check_cancelled(cancellation)?;
        validate_inputs(surface, inputs, step_years, cancellation)?;
        let count = surface.cells().len();
        let mut land_exposure_sum = vec![0.0_f64; count];
        let mut marine_exposure_sum = vec![0.0_f64; count];
        let mut cell_perimeter_m = vec![0.0_f64; count];
        for (position, edge) in surface.edges().iter().enumerate() {
            poll_cancelled(cancellation, position)?;
            let length_m = edge.length.get();
            cell_perimeter_m[edge.cells[0].raw() as usize] += length_m;
            cell_perimeter_m[edge.cells[1].raw() as usize] += length_m;
        }

        for (position, edge) in surface.edges().iter().enumerate() {
            poll_cancelled(cancellation, position)?;
            let first = edge.cells[0].raw() as usize;
            let second = edge.cells[1].raw() as usize;
            let wet_fraction = inputs.wet_edge_fraction[position];
            if wet_fraction == 0.0 {
                continue;
            }
            for (land, ocean) in [(first, second), (second, first)] {
                let land_fraction = 1.0 - inputs.ocean_area_fraction[land];
                let aperture_m = edge.length.get() * wet_fraction * land_fraction;
                if aperture_m == 0.0 {
                    continue;
                }
                let exposure = edge_exposure(edge, land, ocean, inputs);
                let weighted = exposure * aperture_m;
                land_exposure_sum[land] += weighted;
                marine_exposure_sum[ocean] += weighted;
            }
        }

        let land_exposure = land_exposure_sum
            .iter()
            .zip(&cell_perimeter_m)
            .map(|(&sum, &perimeter)| {
                if perimeter > 0.0 {
                    sum / perimeter
                } else {
                    0.0
                }
            })
            .collect::<Vec<_>>();
        let marine_exposure = marine_exposure_sum
            .iter()
            .zip(&cell_perimeter_m)
            .map(|(&sum, &perimeter)| {
                if perimeter > 0.0 {
                    sum / perimeter
                } else {
                    0.0
                }
            })
            .collect::<Vec<_>>();
        let mut coastal_erosion_m = vec![0.0_f64; count];
        let mut removed_by_source_kg = vec![[0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]; count];
        let mut ocean_injection_by_source_kg =
            vec![[0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]; count];
        let mut sediment_stock_removed_by_source_kg =
            vec![[0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]; count];

        for land in 0..count {
            poll_cancelled(cancellation, land)?;
            let exposure = land_exposure[land];
            if exposure == 0.0 {
                continue;
            }
            let area_m2 = surface.cells()[land].area.get();
            let sediment_stock_kg = inputs.sediment_mass_by_source_kg[land].iter().sum::<f64>();
            let sediment_thickness_m =
                sediment_stock_kg / (area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3);
            let shield = 1.0 / (1.0 + sediment_thickness_m / FORMATION_COASTAL_COVER_SHIELD_M);
            let retained_erosion_m = FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR
                * step_years
                * f64::from(inputs.substrate_erodibility[land])
                * shield
                * exposure;
            if retained_erosion_m == 0.0 {
                continue;
            }
            coastal_erosion_m[land] = retained_erosion_m;
            let substrate_source = inputs
                .sediment_sources
                .get(land)
                .expect("validated source field covers every cell")
                .raw() as usize;
            let (sediment_erosion_m, sediment_mass_kg) =
                if retained_erosion_m >= sediment_thickness_m {
                    (sediment_thickness_m, sediment_stock_kg)
                } else {
                    (
                        retained_erosion_m,
                        retained_erosion_m * area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
                    )
                };
            sediment_stock_removed_by_source_kg[land] =
                split_mass_by_weights(sediment_mass_kg, inputs.sediment_mass_by_source_kg[land]);
            removed_by_source_kg[land] = sediment_stock_removed_by_source_kg[land];
            let substrate_mass_kg = (retained_erosion_m - sediment_erosion_m)
                * area_m2
                * f64::from(inputs.substrate_density_kg_m3[land]);
            removed_by_source_kg[land][substrate_source] += substrate_mass_kg;
            let cell = CellId::from_raw(land as u32);
            let mut receivers = Vec::new();
            let mut total_weight = 0.0_f64;
            for &edge_id in surface
                .cell_edges(cell)
                .expect("validated surface covers every cell")
            {
                let edge = surface
                    .edge(edge_id)
                    .expect("validated surface covers every boundary edge");
                let receiver = surface
                    .opposite_cell(cell, edge_id)
                    .expect("closed spherical edge has an opposite cell")
                    .raw() as usize;
                let wet_fraction = inputs.wet_edge_fraction[edge_id.raw() as usize];
                let receiver_ocean_fraction = inputs.ocean_area_fraction[receiver];
                if wet_fraction == 0.0 || receiver_ocean_fraction == 0.0 {
                    continue;
                }
                let land_fraction = 1.0 - inputs.ocean_area_fraction[land];
                let weight = edge.length.get()
                    * wet_fraction
                    * land_fraction
                    * edge_exposure(edge, land, receiver, inputs);
                if weight > 0.0 {
                    receivers.push((receiver, weight));
                    total_weight += weight;
                }
            }
            if total_weight == 0.0 {
                coastal_erosion_m[land] = 0.0;
                sediment_stock_removed_by_source_kg[land] = [0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT];
                removed_by_source_kg[land] = [0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT];
                continue;
            }
            let last = receivers.len() - 1;
            for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
                let source_mass = removed_by_source_kg[land][source];
                let mut remaining = source_mass;
                for (position, &(receiver, weight)) in receivers.iter().enumerate() {
                    let mass = if position == last {
                        remaining
                    } else {
                        source_mass * weight / total_weight
                    };
                    remaining -= mass;
                    ocean_injection_by_source_kg[receiver][source] += mass;
                }
            }
        }

        let mut produced_by_source_kg = [0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT];
        for (index, channels) in ocean_injection_by_source_kg.iter().enumerate() {
            poll_cancelled(cancellation, index)?;
            for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
                produced_by_source_kg[source] += channels[source];
            }
        }
        let produced_mass_kg = produced_by_source_kg.iter().sum();
        check_cancelled(cancellation)?;
        Ok(CoastalExchangeStep {
            coastal_erosion_m,
            land_exposure,
            marine_exposure,
            removed_by_source_kg,
            ocean_injection_by_source_kg,
            produced_mass_kg,
            produced_by_source_kg,
            sediment_stock_removed_by_source_kg,
        })
    }
}

fn edge_exposure(
    edge: &crate::world::spatial::SphericalSurfaceEdge,
    land: usize,
    ocean: usize,
    inputs: CoastalInputs<'_>,
) -> f64 {
    let normal = edge.normal_from_first.components();
    let midpoint = edge.midpoint.components();
    let alongshore = cross(midpoint, normal);
    let mut normal_wind_square = 0.0_f64;
    let mut alongshore_current_square = 0.0_f64;
    for month in 0..CLIMATE_MONTH_COUNT {
        let wind = inputs.near_surface_wind_m_s[land][month].map(f64::from);
        let current = inputs.surface_ocean_current_m_s[ocean][month].map(f64::from);
        let wind_component = dot(wind, normal).abs();
        let current_component = dot(current, alongshore).abs();
        normal_wind_square += wind_component * wind_component;
        alongshore_current_square += current_component * current_component;
    }
    let wind_rms = (normal_wind_square / CLIMATE_MONTH_COUNT as f64).sqrt();
    let current_rms = (alongshore_current_square / CLIMATE_MONTH_COUNT as f64).sqrt();
    let forcing = ((wind_rms / FORMATION_COASTAL_WIND_REFERENCE_M_S).powi(2)
        + (current_rms / FORMATION_COASTAL_CURRENT_REFERENCE_M_S).powi(2))
    .sqrt();
    forcing / (1.0 + forcing)
}

fn validate_inputs(
    surface: &SphericalSurfaceSnapshot,
    inputs: CoastalInputs<'_>,
    step_years: f64,
    cancellation: &BuildCancellation,
) -> Result<(), CoastGenerationError> {
    if !step_years.is_finite() || step_years <= 0.0 {
        return Err(CoastGenerationError::InvalidStepYears { found: step_years });
    }
    let count = surface.cells().len();
    for (field, found) in [
        ("elevation_m", inputs.elevation_m.len()),
        ("ocean_area_fraction", inputs.ocean_area_fraction.len()),
        ("substrate_erodibility", inputs.substrate_erodibility.len()),
        (
            "sediment_mass_by_source_kg",
            inputs.sediment_mass_by_source_kg.len(),
        ),
        (
            "substrate_density_kg_m3",
            inputs.substrate_density_kg_m3.len(),
        ),
        ("sediment_sources", inputs.sediment_sources.len()),
        ("near_surface_wind_m_s", inputs.near_surface_wind_m_s.len()),
        (
            "surface_ocean_current_m_s",
            inputs.surface_ocean_current_m_s.len(),
        ),
    ] {
        if found != count {
            return Err(CoastGenerationError::CellCountMismatch {
                field,
                expected: count,
                found,
            });
        }
    }
    if inputs.wet_edge_fraction.len() != surface.edges().len() {
        return Err(CoastGenerationError::CellCountMismatch {
            field: "wet_edge_fraction",
            expected: surface.edges().len(),
            found: inputs.wet_edge_fraction.len(),
        });
    }
    for index in 0..count {
        poll_cancelled(cancellation, index)?;
        let elevation_m = inputs.elevation_m[index];
        if !elevation_m.is_finite()
            || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M)).contains(&elevation_m)
        {
            return Err(CoastGenerationError::InvalidCellValue {
                field: "elevation_m",
                cell: CellId::from_raw(index as u32),
                found: elevation_m,
            });
        }
        let ocean_area_fraction = inputs.ocean_area_fraction[index];
        if !ocean_area_fraction.is_finite() || !(0.0..=1.0).contains(&ocean_area_fraction) {
            return Err(CoastGenerationError::InvalidCellValue {
                field: "ocean_area_fraction",
                cell: CellId::from_raw(index as u32),
                found: ocean_area_fraction,
            });
        }
        for (field, value, minimum, maximum) in [
            (
                "substrate_erodibility",
                inputs.substrate_erodibility[index],
                0.0,
                1.0,
            ),
            (
                "substrate_density_kg_m3",
                inputs.substrate_density_kg_m3[index],
                CRUST_DENSITY_MIN_KG_M3,
                CRUST_DENSITY_MAX_KG_M3,
            ),
        ] {
            if !value.is_finite() || !(minimum..=maximum).contains(&value) {
                return Err(CoastGenerationError::InvalidCellValue {
                    field,
                    cell: CellId::from_raw(index as u32),
                    found: f64::from(value),
                });
            }
        }
        let sediment_mass_kg = inputs.sediment_mass_by_source_kg[index]
            .iter()
            .copied()
            .sum::<f64>();
        if !sediment_mass_kg.is_finite()
            || inputs.sediment_mass_by_source_kg[index]
                .iter()
                .any(|&value| !value.is_finite() || value < 0.0)
        {
            return Err(CoastGenerationError::InvalidCellValue {
                field: "sediment_mass_by_source_kg",
                cell: CellId::from_raw(index as u32),
                found: sediment_mass_kg,
            });
        }
        for (field, months) in [
            (
                "near_surface_wind_m_s",
                &inputs.near_surface_wind_m_s[index],
            ),
            (
                "surface_ocean_current_m_s",
                &inputs.surface_ocean_current_m_s[index],
            ),
        ] {
            if let Some(value) = months
                .iter()
                .flat_map(|vector| vector.iter())
                .find(|value| !value.is_finite())
            {
                return Err(CoastGenerationError::InvalidCellValue {
                    field,
                    cell: CellId::from_raw(index as u32),
                    found: f64::from(*value),
                });
            }
        }
    }
    for (index, &wet_edge_fraction) in inputs.wet_edge_fraction.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        if !wet_edge_fraction.is_finite() || !(0.0..=1.0).contains(&wet_edge_fraction) {
            return Err(CoastGenerationError::InvalidEdgeValue {
                field: "wet_edge_fraction",
                edge: index,
                found: wet_edge_fraction,
            });
        }
    }
    Ok(())
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn poll_cancelled(
    cancellation: &BuildCancellation,
    index: usize,
) -> Result<(), CoastGenerationError> {
    if index & CANCELLATION_POLL_MASK == 0 {
        check_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), CoastGenerationError> {
    if cancellation.is_cancelled() {
        Err(CoastGenerationError::Cancelled)
    } else {
        Ok(())
    }
}

fn map_surface_error(
    error: SphericalSurfaceValidationError,
    cancellation: &BuildCancellation,
) -> CoastGenerationError {
    if cancellation.is_cancelled() {
        CoastGenerationError::Cancelled
    } else {
        CoastGenerationError::InvalidSurface(error)
    }
}

#[derive(Debug, Error)]
pub enum CoastGenerationError {
    #[error("coastal exchange cancelled")]
    Cancelled,
    #[error("invalid authoritative surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    #[error("coastal field {field} has length {found}; expected {expected}")]
    CellCountMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("coastal field {field} has invalid value {found} at {cell:?}")]
    InvalidCellValue {
        field: &'static str,
        cell: CellId,
        found: f64,
    },
    #[error("coastal edge field {field} has invalid value {found} at edge {edge}")]
    InvalidEdgeValue {
        field: &'static str,
        edge: usize,
        found: f64,
    },
    #[error("coastal step duration must be finite and positive, got {found}")]
    InvalidStepYears { found: f64 },
}
