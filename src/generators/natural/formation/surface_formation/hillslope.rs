use thiserror::Error;

use crate::engine::BuildCancellation;
use crate::world::natural::{
    formation_elevation_from_components, SedimentSourceKindField, SurfaceWaterField,
    SurfaceWaterKind, CRUST_DENSITY_MAX_KG_M3, CRUST_DENSITY_MIN_KG_M3, ELEVATION_MAX_M,
    ELEVATION_MIN_M, FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3, FORMATION_HILLSLOPE_CRITICAL_SLOPE,
    FORMATION_HILLSLOPE_DENOMINATOR_MIN, FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR,
    FORMATION_HILLSLOPE_ERODIBILITY_BASE, FORMATION_HILLSLOPE_ERODIBILITY_RANGE,
    FORMATION_HILLSLOPE_FRACTURE_BASE, FORMATION_HILLSLOPE_FRACTURE_RANGE,
    FORMATION_HILLSLOPE_PRECIPITATION_FACTOR_MAX, FORMATION_HILLSLOPE_PRECIPITATION_REFERENCE_MM,
    FORMATION_HILLSLOPE_WEATHERING_BASE, FORMATION_HILLSLOPE_WEATHERING_RANGE,
    SEDIMENT_PROVENANCE_SOURCE_COUNT,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SphericalSurfaceValidationError};
use crate::world::CellId;

use super::sediment::split_mass_by_weights;

const CANCELLATION_POLL_MASK: usize = 255;

/// Borrowed physical fields consumed by one nonlinear hillslope step.
#[derive(Debug, Clone, Copy)]
pub struct HillslopeInputs<'a> {
    pub elevation_m: &'a [f64],
    pub surface_water: &'a SurfaceWaterField,
    pub substrate_erodibility: &'a [f32],
    pub fracture_intensity: &'a [f32],
    pub annual_precipitation_mm: &'a [f32],
    pub substrate_density_kg_m3: &'a [f32],
    pub sediment_sources: &'a SedimentSourceKindField,
    pub sediment_mass_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
}

#[derive(Debug, Clone, Copy)]
struct EdgeTransfer {
    donor: usize,
    receiver: usize,
    mass_kg: f64,
}

/// Reusable dense buffers for the paired edge solve.
#[derive(Debug, Default)]
pub struct HillslopeWorkspace {
    transfers: Vec<EdgeTransfer>,
    stability_diagonal_per_year: Vec<f64>,
    erosion_mass_kg: Vec<f64>,
    deposition_mass_kg: Vec<f64>,
    erosion_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    deposition_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    allocation_epoch: u64,
}

impl HillslopeWorkspace {
    fn prepare(&mut self, cell_count: usize, edge_count: usize) {
        let mut allocated = false;
        if self.transfers.capacity() < edge_count {
            self.transfers
                .reserve_exact(edge_count - self.transfers.capacity());
            allocated = true;
        }
        self.transfers.clear();
        for values in [
            &mut self.stability_diagonal_per_year,
            &mut self.erosion_mass_kg,
            &mut self.deposition_mass_kg,
        ] {
            if values.len() != cell_count {
                values.resize(cell_count, 0.0);
                allocated = true;
            }
            values.fill(0.0);
        }
        if self.erosion_by_source_kg.len() != cell_count {
            self.erosion_by_source_kg
                .resize(cell_count, [0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]);
            allocated = true;
        }
        if self.deposition_by_source_kg.len() != cell_count {
            self.deposition_by_source_kg
                .resize(cell_count, [0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]);
            allocated = true;
        }
        self.erosion_by_source_kg
            .fill([0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]);
        self.deposition_by_source_kg
            .fill([0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]);
        if allocated {
            self.allocation_epoch = self.allocation_epoch.saturating_add(1);
        }
    }

    /// Monotonic testable marker incremented only when dense capacity grows.
    pub const fn allocation_epoch(&self) -> u64 {
        self.allocation_epoch
    }
}

/// Retained output and paired-volume evidence for one hillslope step.
#[derive(Debug, Clone, PartialEq)]
pub struct HillslopeTransportStep {
    elevation_m: Vec<f64>,
    hillslope_erosion_m: Vec<f64>,
    hillslope_deposition_m: Vec<f64>,
    removed_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    deposited_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    transported_mass_kg: f64,
    removed_volume_m3: f64,
    deposited_volume_m3: f64,
    removed_mass_kg: f64,
    deposited_mass_kg: f64,
    retained_mass_relative_error: f64,
    sediment_stock_removed_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
}

impl HillslopeTransportStep {
    pub fn elevation_m(&self) -> &[f64] {
        &self.elevation_m
    }

    pub fn hillslope_erosion_m(&self) -> &[f64] {
        &self.hillslope_erosion_m
    }

    pub fn hillslope_deposition_m(&self) -> &[f64] {
        &self.hillslope_deposition_m
    }

    pub fn deposited_by_source_kg(&self) -> &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.deposited_by_source_kg
    }

    pub fn removed_by_source_kg(&self) -> &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.removed_by_source_kg
    }

    pub const fn transported_mass_kg(&self) -> f64 {
        self.transported_mass_kg
    }

    pub const fn removed_volume_m3(&self) -> f64 {
        self.removed_volume_m3
    }

    pub const fn deposited_volume_m3(&self) -> f64 {
        self.deposited_volume_m3
    }

    pub const fn removed_mass_kg(&self) -> f64 {
        self.removed_mass_kg
    }

    pub const fn deposited_mass_kg(&self) -> f64 {
        self.deposited_mass_kg
    }

    pub const fn retained_mass_relative_error(&self) -> f64 {
        self.retained_mass_relative_error
    }

    pub fn sediment_stock_removed_by_source_kg(
        &self,
    ) -> &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.sediment_stock_removed_by_source_kg
    }
}

/// Paired finite-volume Roering-style coarse hillslope transport.
#[derive(Debug, Clone, Copy, Default)]
pub struct NonlinearHillslopeTransport;

impl NonlinearHillslopeTransport {
    pub fn advance(
        surface: &SphericalSurfaceSnapshot,
        inputs: HillslopeInputs<'_>,
        step_years: f64,
        workspace: &mut HillslopeWorkspace,
        cancellation: &BuildCancellation,
    ) -> Result<HillslopeTransportStep, HillslopeGenerationError> {
        check_cancelled(cancellation)?;
        surface.validate()?;
        Self::advance_from_validated_surface(surface, inputs, step_years, workspace, cancellation)
    }

    /// Returns the monotone explicit step bound for a validated production surface.
    pub(super) fn maximum_stable_step_years_from_validated_surface(
        surface: &SphericalSurfaceSnapshot,
        inputs: HillslopeInputs<'_>,
        workspace: &mut HillslopeWorkspace,
        cancellation: &BuildCancellation,
    ) -> Result<f64, HillslopeGenerationError> {
        check_cancelled(cancellation)?;
        validate_inputs(surface, inputs, 1.0, cancellation)?;
        workspace.prepare(surface.cells().len(), surface.edges().len());
        maximum_monotone_step_years(
            surface,
            inputs,
            &mut workspace.stability_diagonal_per_year,
            cancellation,
        )
    }

    /// Same paired transfer for a caller that already validated the surface.
    pub(super) fn advance_from_validated_surface(
        surface: &SphericalSurfaceSnapshot,
        inputs: HillslopeInputs<'_>,
        step_years: f64,
        workspace: &mut HillslopeWorkspace,
        cancellation: &BuildCancellation,
    ) -> Result<HillslopeTransportStep, HillslopeGenerationError> {
        check_cancelled(cancellation)?;
        validate_inputs(surface, inputs, step_years, cancellation)?;
        let cell_count = surface.cells().len();
        workspace.prepare(cell_count, surface.edges().len());
        let maximum_step_years = maximum_monotone_step_years(
            surface,
            inputs,
            &mut workspace.stability_diagonal_per_year,
            cancellation,
        )?;
        if step_years > maximum_step_years {
            return Err(HillslopeGenerationError::UnstableStep {
                found: step_years,
                maximum: maximum_step_years,
            });
        }
        let trace = std::env::var_os("SEKAI_P5_TRACE").is_some();

        for position in 0..surface.edges().len() {
            poll_cancelled(cancellation, position)?;
            let Some(transport) = edge_transport(surface, inputs, position) else {
                continue;
            };
            let mass_kg = transport.transmissibility_m2_per_year
                * transport.relief_m
                * step_years
                * f64::from(inputs.substrate_density_kg_m3[transport.donor]);
            workspace.transfers.push(EdgeTransfer {
                donor: transport.donor,
                receiver: transport.receiver,
                mass_kg,
            });
            workspace.erosion_mass_kg[transport.donor] += mass_kg;
            workspace.deposition_mass_kg[transport.receiver] += mass_kg;
        }

        let transported_mass_kg = workspace
            .transfers
            .iter()
            .map(|transfer| transfer.mass_kg)
            .sum::<f64>();

        let mut sediment_stock_removed_by_source_kg = Vec::with_capacity(cell_count);
        let mut source_fractions = vec![[0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]; cell_count];
        for (index, source_fraction) in source_fractions.iter_mut().enumerate() {
            poll_cancelled(cancellation, index)?;
            let removed = workspace.erosion_mass_kg[index];
            let stock_by_source_kg = inputs.sediment_mass_by_source_kg[index];
            let stock = stock_by_source_kg.iter().sum::<f64>();
            let stock_removed = stock.min(removed);
            let stock_removed_by_source_kg =
                split_mass_by_weights(stock_removed, stock_by_source_kg);
            sediment_stock_removed_by_source_kg.push(stock_removed_by_source_kg);
            if removed == 0.0 {
                continue;
            }
            for (fraction, stock_mass_kg) in
                source_fraction.iter_mut().zip(stock_removed_by_source_kg)
            {
                *fraction = stock_mass_kg / removed;
            }
            let substrate_source = inputs
                .sediment_sources
                .get(index)
                .expect("validated source field covers every cell")
                .raw() as usize;
            source_fraction[substrate_source] += (removed - stock_removed) / removed;
            let source_sum = source_fraction.iter().sum::<f64>();
            for fraction in source_fraction {
                *fraction /= source_sum;
            }
        }
        for (position, transfer) in workspace.transfers.iter().enumerate() {
            poll_cancelled(cancellation, position)?;
            let by_source =
                split_mass_by_weights(transfer.mass_kg, source_fractions[transfer.donor]);
            for (source, mass_kg) in by_source.into_iter().enumerate() {
                workspace.erosion_by_source_kg[transfer.donor][source] += mass_kg;
                workspace.deposition_by_source_kg[transfer.receiver][source] += mass_kg;
            }
        }

        let mut hillslope_erosion_m = Vec::with_capacity(cell_count);
        let mut hillslope_deposition_m = Vec::with_capacity(cell_count);
        let mut elevation_m = Vec::with_capacity(cell_count);
        let mut retained_removed_mass_kg = 0.0_f64;
        let mut retained_deposited_mass_kg = 0.0_f64;
        let mut removed_volume_m3 = 0.0_f64;
        for index in 0..sediment_stock_removed_by_source_kg.len() {
            poll_cancelled(cancellation, index)?;
            let stock_removed_kg = inputs.sediment_mass_by_source_kg[index]
                .iter()
                .sum::<f64>()
                .min(workspace.erosion_mass_kg[index]);
            let area_m2 = surface.cells()[index].area.get();
            let substrate_removed_kg = workspace.erosion_mass_kg[index] - stock_removed_kg;
            let erosion_volume_m3 = stock_removed_kg / FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3
                + substrate_removed_kg / f64::from(inputs.substrate_density_kg_m3[index]);
            let erosion_m = erosion_volume_m3 / area_m2;
            let deposition_m = workspace.deposition_mass_kg[index]
                / (FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3 * area_m2);
            removed_volume_m3 += erosion_volume_m3;
            retained_removed_mass_kg += workspace.erosion_mass_kg[index];
            retained_deposited_mass_kg += workspace.deposition_mass_kg[index];
            hillslope_erosion_m.push(erosion_m);
            hillslope_deposition_m.push(deposition_m);
            elevation_m.push(formation_elevation_from_components(
                inputs.elevation_m[index],
                0.0,
                0.0,
                erosion_m,
                deposition_m,
                0.0,
                0.0,
                0.0,
                0.0,
            ));
        }
        validate_no_new_extrema(inputs, &elevation_m)?;
        let retained_scale = retained_removed_mass_kg
            .abs()
            .max(retained_deposited_mass_kg.abs())
            .max(1.0);
        let retained_mass_relative_error =
            (retained_removed_mass_kg - retained_deposited_mass_kg).abs() / retained_scale;
        if trace {
            eprintln!(
                "[p5-hillslope] physical_step={step_years:.3} stable_step_max={maximum_step_years:.3} active_edges={} transported_mass={transported_mass_kg:.6e}",
                workspace.transfers.len(),
            );
        }
        check_cancelled(cancellation)?;
        Ok(HillslopeTransportStep {
            elevation_m,
            hillslope_erosion_m,
            hillslope_deposition_m,
            removed_by_source_kg: workspace.erosion_by_source_kg.clone(),
            deposited_by_source_kg: workspace.deposition_by_source_kg.clone(),
            transported_mass_kg,
            removed_volume_m3,
            deposited_volume_m3: transported_mass_kg / FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
            removed_mass_kg: transported_mass_kg,
            deposited_mass_kg: transported_mass_kg,
            retained_mass_relative_error,
            sediment_stock_removed_by_source_kg,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct EdgeTransport {
    donor: usize,
    receiver: usize,
    relief_m: f64,
    transmissibility_m2_per_year: f64,
}

fn edge_transport(
    surface: &SphericalSurfaceSnapshot,
    inputs: HillslopeInputs<'_>,
    edge_index: usize,
) -> Option<EdgeTransport> {
    let edge = &surface.edges()[edge_index];
    let first = edge.cells[0].raw() as usize;
    let second = edge.cells[1].raw() as usize;
    if inputs.surface_water.get(first) != Some(SurfaceWaterKind::DryLand)
        || inputs.surface_water.get(second) != Some(SurfaceWaterKind::DryLand)
    {
        return None;
    }
    let first_height = inputs.elevation_m[first];
    let second_height = inputs.elevation_m[second];
    let (donor, receiver, relief_m) = if first_height > second_height {
        (first, second, first_height - second_height)
    } else if second_height > first_height {
        (second, first, second_height - first_height)
    } else {
        return None;
    };
    let slope = relief_m / edge.center_distance.get();
    let normalized_slope = slope / FORMATION_HILLSLOPE_CRITICAL_SLOPE;
    let denominator =
        (1.0 - normalized_slope * normalized_slope).max(FORMATION_HILLSLOPE_DENOMINATOR_MIN);
    let effective_diffusivity = FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR
        * 0.5
        * (hillslope_factor(inputs, donor) + hillslope_factor(inputs, receiver));
    Some(EdgeTransport {
        donor,
        receiver,
        relief_m,
        transmissibility_m2_per_year: effective_diffusivity * edge.length.get()
            / (edge.center_distance.get() * denominator),
    })
}

/// Monotone forward-Euler bound for the irregular finite-volume operator.
fn maximum_monotone_step_years(
    surface: &SphericalSurfaceSnapshot,
    inputs: HillslopeInputs<'_>,
    diagonal_per_year: &mut [f64],
    cancellation: &BuildCancellation,
) -> Result<f64, HillslopeGenerationError> {
    diagonal_per_year.fill(0.0);
    for (position, edge) in surface.edges().iter().enumerate() {
        poll_cancelled(cancellation, position)?;
        let Some(transport) = edge_transport(surface, inputs, position) else {
            continue;
        };
        let first = edge.cells[0].raw() as usize;
        let second = edge.cells[1].raw() as usize;
        let density_ratio = f64::from(
            inputs.substrate_density_kg_m3[first].max(inputs.substrate_density_kg_m3[second]),
        ) / FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3;
        let bounded_transmissibility = transport.transmissibility_m2_per_year * density_ratio;
        diagonal_per_year[first] += bounded_transmissibility / surface.cells()[first].area.get();
        diagonal_per_year[second] += bounded_transmissibility / surface.cells()[second].area.get();
    }
    let maximum_diagonal = diagonal_per_year.iter().copied().fold(0.0_f64, f64::max);
    Ok(if maximum_diagonal == 0.0 {
        f64::INFINITY
    } else {
        maximum_diagonal.recip()
    })
}

fn hillslope_factor(inputs: HillslopeInputs<'_>, index: usize) -> f64 {
    let erodibility = FORMATION_HILLSLOPE_ERODIBILITY_BASE
        + FORMATION_HILLSLOPE_ERODIBILITY_RANGE * f64::from(inputs.substrate_erodibility[index]);
    let fracture = FORMATION_HILLSLOPE_FRACTURE_BASE
        + FORMATION_HILLSLOPE_FRACTURE_RANGE * f64::from(inputs.fracture_intensity[index]);
    let wetness = (f64::from(inputs.annual_precipitation_mm[index])
        / FORMATION_HILLSLOPE_PRECIPITATION_REFERENCE_MM)
        .clamp(0.0, FORMATION_HILLSLOPE_PRECIPITATION_FACTOR_MAX)
        .sqrt()
        / FORMATION_HILLSLOPE_PRECIPITATION_FACTOR_MAX.sqrt();
    let weathering =
        FORMATION_HILLSLOPE_WEATHERING_BASE + FORMATION_HILLSLOPE_WEATHERING_RANGE * wetness;
    erodibility * fracture * weathering
}

fn validate_inputs(
    surface: &SphericalSurfaceSnapshot,
    inputs: HillslopeInputs<'_>,
    step_years: f64,
    cancellation: &BuildCancellation,
) -> Result<(), HillslopeGenerationError> {
    if !step_years.is_finite() || step_years <= 0.0 {
        return Err(HillslopeGenerationError::InvalidStepYears { found: step_years });
    }
    let count = surface.cells().len();
    for (field, found) in [
        ("elevation_m", inputs.elevation_m.len()),
        ("surface_water", inputs.surface_water.len()),
        ("substrate_erodibility", inputs.substrate_erodibility.len()),
        ("fracture_intensity", inputs.fracture_intensity.len()),
        (
            "annual_precipitation_mm",
            inputs.annual_precipitation_mm.len(),
        ),
        (
            "substrate_density_kg_m3",
            inputs.substrate_density_kg_m3.len(),
        ),
        ("sediment_sources", inputs.sediment_sources.len()),
        (
            "sediment_mass_by_source_kg",
            inputs.sediment_mass_by_source_kg.len(),
        ),
    ] {
        if found != count {
            return Err(HillslopeGenerationError::CellCountMismatch {
                field,
                expected: count,
                found,
            });
        }
    }
    for index in 0..count {
        poll_cancelled(cancellation, index)?;
        let cell = CellId::from_raw(index as u32);
        let elevation = inputs.elevation_m[index];
        if !elevation.is_finite()
            || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M)).contains(&elevation)
        {
            return Err(HillslopeGenerationError::InvalidCellValue {
                field: "elevation_m",
                cell,
                found: elevation,
            });
        }
        for (field, value, maximum) in [
            (
                "substrate_erodibility",
                inputs.substrate_erodibility[index],
                1.0,
            ),
            ("fracture_intensity", inputs.fracture_intensity[index], 1.0),
            (
                "annual_precipitation_mm",
                inputs.annual_precipitation_mm[index],
                f32::MAX,
            ),
            (
                "substrate_density_kg_m3",
                inputs.substrate_density_kg_m3[index],
                CRUST_DENSITY_MAX_KG_M3,
            ),
        ] {
            let minimum = if field == "substrate_density_kg_m3" {
                CRUST_DENSITY_MIN_KG_M3
            } else {
                0.0
            };
            if !value.is_finite() || !(minimum..=maximum).contains(&value) {
                return Err(HillslopeGenerationError::InvalidCellValue {
                    field,
                    cell,
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
            return Err(HillslopeGenerationError::InvalidCellValue {
                field: "sediment_mass_by_source_kg",
                cell,
                found: sediment_mass_kg,
            });
        }
    }
    Ok(())
}

fn validate_no_new_extrema(
    inputs: HillslopeInputs<'_>,
    result_elevation_m: &[f64],
) -> Result<(), HillslopeGenerationError> {
    let minimum = inputs
        .elevation_m
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let maximum = inputs
        .elevation_m
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    if let Some((index, &found)) = result_elevation_m
        .iter()
        .enumerate()
        .find(|(_, value)| **value < minimum || **value > maximum)
    {
        return Err(HillslopeGenerationError::NewExtremum {
            cell: CellId::from_raw(index as u32),
            found,
            minimum,
            maximum,
        });
    }
    Ok(())
}

fn poll_cancelled(
    cancellation: &BuildCancellation,
    index: usize,
) -> Result<(), HillslopeGenerationError> {
    if index & CANCELLATION_POLL_MASK == 0 {
        check_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), HillslopeGenerationError> {
    if cancellation.is_cancelled() {
        Err(HillslopeGenerationError::Cancelled)
    } else {
        Ok(())
    }
}

/// Failures returned by paired P5 hillslope transport.
#[derive(Debug, Error)]
pub enum HillslopeGenerationError {
    #[error("hillslope transport cancelled")]
    Cancelled,
    #[error("invalid authoritative surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    #[error("hillslope field {field} has length {found}; expected {expected}")]
    CellCountMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("hillslope field {field} has invalid value {found} at {cell:?}")]
    InvalidCellValue {
        field: &'static str,
        cell: CellId,
        found: f64,
    },
    #[error("hillslope step duration must be finite and positive, got {found}")]
    InvalidStepYears { found: f64 },
    #[error("hillslope step {found} years exceeds monotone finite-volume limit {maximum} years")]
    UnstableStep { found: f64, maximum: f64 },
    #[error(
        "hillslope transport created elevation {found} at {cell:?} outside input extrema \
         [{minimum}, {maximum}]"
    )]
    NewExtremum {
        cell: CellId,
        found: f64,
        minimum: f64,
        maximum: f64,
    },
}
