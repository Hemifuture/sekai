use std::cmp::Reverse;
use std::collections::BinaryHeap;

use thiserror::Error;

use crate::world::natural::{
    ElevationField, HydroErosionSpec, HydroErosionSpecError, HydrologySnapshot,
    HydrologyValidationError, LandOceanKind, ReliefSnapshot, ReliefValidationError,
    SurfaceProcessSnapshot, SurfaceProcessValidationError, SurfaceWaterKind, ELEVATION_MAX_M,
    ELEVATION_MIN_M, MAX_DEPOSITION_THICKNESS_M, MAX_EROSION_DEPTH_M, SURFACE_PROCESS_SCHEMA_V1,
};
use crate::world::spatial::{SpatialSnapshot, SpatialValidationError, Topology};
use crate::world::CellId;

const MAX_FORMATION_INCISION_M: f32 = 300.0;
const MAX_LOCAL_DEPOSITION_M: f32 = 50.0;
const DISCHARGE_HALF_RESPONSE_M3_S: f32 = 50.0;
const SLOPE_HALF_RESPONSE: f32 = 0.01;
const CENTIMETERS_PER_METER: f64 = 100.0;

/// Bounded current-slice stream-power incision and conservative sediment routing.
#[derive(Debug, Clone, Copy, Default)]
pub struct FluvialErosionGenerator;

impl FluvialErosionGenerator {
    /// Forms current surface processes from first-pass hydrology and erosion resistance.
    pub fn generate(
        spatial: &SpatialSnapshot,
        relief: &ReliefSnapshot,
        erosion_resistance: &[f32],
        hydrology: &HydrologySnapshot,
        spec: &HydroErosionSpec,
    ) -> Result<SurfaceProcessSnapshot, FluvialErosionError> {
        validate_inputs(spatial, relief, erosion_resistance, hydrology, spec)?;
        let energy = stream_energy(spatial, hydrology);
        let erosion_depth_m = incision(relief, erosion_resistance, &energy, spec);
        let order = upstream_to_downstream_order(hydrology.flow_receiver())?;
        let sediment = route_sediment(
            spatial,
            relief,
            hydrology,
            &order,
            &energy,
            &erosion_depth_m,
        );
        let surface_values = relief
            .elevation_m()
            .values()
            .iter()
            .zip(&erosion_depth_m)
            .zip(&sediment.deposition_thickness_m)
            .map(|((&constructional, &erosion), &deposition)| {
                if erosion == 0.0 && deposition == 0.0 {
                    constructional
                } else {
                    constructional - erosion + deposition
                }
            })
            .collect();

        let snapshot = SurfaceProcessSnapshot::new(
            SURFACE_PROCESS_SCHEMA_V1,
            relief.cell_count(),
            erosion_depth_m,
            sediment.deposition_thickness_m,
            ElevationField::from_values(surface_values)?,
            sediment.throughput_m3,
            sediment.export_m3,
        )?;
        snapshot.validate_against(spatial, relief)?;
        Ok(snapshot)
    }
}

fn validate_inputs(
    spatial: &SpatialSnapshot,
    relief: &ReliefSnapshot,
    erosion_resistance: &[f32],
    hydrology: &HydrologySnapshot,
    spec: &HydroErosionSpec,
) -> Result<(), FluvialErosionError> {
    spatial.validate()?;
    relief.validate_against(spatial)?;
    hydrology.validate_against_spatial(spatial)?;
    spec.validate()?;
    if erosion_resistance.len() != spatial.cell_count() {
        return Err(FluvialErosionError::CellCountMismatch {
            input: "erosion_resistance",
            expected: spatial.cell_count(),
            found: erosion_resistance.len(),
        });
    }
    if hydrology.cell_count() != relief.cell_count() {
        return Err(FluvialErosionError::CellCountMismatch {
            input: "hydrology",
            expected: relief.cell_count() as usize,
            found: hydrology.cell_count() as usize,
        });
    }
    for (index, &found) in erosion_resistance.iter().enumerate() {
        if !found.is_finite() || !(0.0..=1.0).contains(&found) {
            return Err(FluvialErosionError::ResistanceOutOfRange {
                cell: CellId::from_raw(index as u32),
                found,
            });
        }
    }
    for index in 0..spatial.cell_count() {
        let cell = CellId::from_raw(index as u32);
        let expected_ocean = relief.land_ocean_kind(cell) == Some(LandOceanKind::Ocean);
        let stored_ocean = hydrology.surface_water().get(index) == Some(SurfaceWaterKind::Ocean);
        if expected_ocean != stored_ocean {
            return Err(FluvialErosionError::OceanClassificationMismatch { cell });
        }
    }
    Ok(())
}

fn stream_energy(spatial: &SpatialSnapshot, hydrology: &HydrologySnapshot) -> Vec<f32> {
    let drainage_surface = hydrology.drainage_surface_elevation_m().values();
    hydrology
        .flow_receiver()
        .iter()
        .enumerate()
        .map(|(index, receiver)| {
            let Some(receiver) = receiver else {
                return 0.0;
            };
            let drop_m = drainage_surface[index] - drainage_surface[receiver.raw() as usize];
            if drop_m <= 0.0 {
                return 0.0;
            }
            let distance_m = spatial
                .distance_between_sites(CellId::from_raw(index as u32), *receiver)
                .expect("validated receiver is a real spatial neighbor")
                .get() as f32;
            let slope = drop_m / distance_m;
            let discharge = hydrology.mean_annual_discharge_m3_s()[index];
            if slope <= 0.0 || discharge <= 0.0 {
                return 0.0;
            }
            let discharge_response = discharge / (discharge + DISCHARGE_HALF_RESPONSE_M3_S);
            let slope_response = slope / (slope + SLOPE_HALF_RESPONSE);
            (discharge_response * slope_response).sqrt().clamp(0.0, 1.0)
        })
        .collect()
}

fn incision(
    relief: &ReliefSnapshot,
    erosion_resistance: &[f32],
    energy: &[f32],
    spec: &HydroErosionSpec,
) -> Vec<f32> {
    let strength = spec.erosion_strength();
    (0..relief.cell_count() as usize)
        .map(|index| {
            let cell = CellId::from_raw(index as u32);
            if strength == 0.0
                || energy[index] == 0.0
                || relief.land_ocean_kind(cell) == Some(LandOceanKind::Ocean)
            {
                return 0.0;
            }
            let constructional = relief.elevation_m().values()[index];
            let lower_bound = (constructional - ELEVATION_MIN_M).max(0.0);
            let hard_cap = MAX_EROSION_DEPTH_M.min(lower_bound);
            let raw = MAX_FORMATION_INCISION_M
                * strength
                * energy[index]
                * (1.0 - erosion_resistance[index]);
            quantize_nearest_centimeter(raw.min(hard_cap)).min(hard_cap)
        })
        .collect()
}

struct SedimentRouting {
    deposition_thickness_m: Vec<f32>,
    throughput_m3: Vec<f64>,
    export_m3: f64,
}

fn route_sediment(
    spatial: &SpatialSnapshot,
    relief: &ReliefSnapshot,
    hydrology: &HydrologySnapshot,
    order: &[CellId],
    energy: &[f32],
    erosion_depth_m: &[f32],
) -> SedimentRouting {
    let cell_count = spatial.cell_count();
    let mut incoming_m3 = vec![0.0_f64; cell_count];
    let mut deposition_thickness_m = vec![0.0_f32; cell_count];
    let mut throughput_m3 = vec![0.0_f64; cell_count];
    let mut export_m3 = 0.0;

    for &cell in order {
        let index = cell.raw() as usize;
        let area_m2 = spatial
            .cell(cell)
            .expect("validated spatial input contains every routed cell")
            .area
            .get();
        let local_eroded_m3 = area_m2 * f64::from(erosion_depth_m[index]);
        let available_m3 = incoming_m3[index] + local_eroded_m3;
        let is_ocean = relief.land_ocean_kind(cell) == Some(LandOceanKind::Ocean);
        let water = hydrology
            .surface_water()
            .get(index)
            .expect("validated water field decodes");
        let terminal = hydrology.flow_receiver()[index].is_none();

        let deposited_m3 = if available_m3 == 0.0 || is_ocean {
            0.0
        } else {
            let retained_fraction = (0.10
                + 0.45 * (1.0 - energy[index])
                + if water == SurfaceWaterKind::Lake {
                    0.25
                } else {
                    0.0
                }
                + if terminal { 0.25 } else { 0.0 })
            .clamp(0.0, 0.95);
            let capacity_response = if water == SurfaceWaterKind::Lake || terminal {
                1.0
            } else {
                0.20 + 0.80 * (1.0 - energy[index])
            };
            let post_erosion = relief.elevation_m().values()[index] - erosion_depth_m[index];
            let elevation_room = (ELEVATION_MAX_M - post_erosion).max(0.0);
            let capacity_m = (MAX_LOCAL_DEPOSITION_M * capacity_response)
                .min(MAX_DEPOSITION_THICKNESS_M)
                .min(elevation_room);
            let desired_m = (available_m3 * f64::from(retained_fraction) / area_m2) as f32;
            let stored_depth = quantize_down_centimeter(desired_m.min(capacity_m));
            deposition_thickness_m[index] = stored_depth;
            area_m2 * f64::from(stored_depth)
        };
        let outgoing_m3 = (available_m3 - deposited_m3).max(0.0);
        throughput_m3[index] = outgoing_m3;
        if let Some(receiver) = hydrology.flow_receiver()[index] {
            incoming_m3[receiver.raw() as usize] += outgoing_m3;
        } else {
            export_m3 += outgoing_m3;
        }
    }

    SedimentRouting {
        deposition_thickness_m,
        throughput_m3,
        export_m3,
    }
}

fn upstream_to_downstream_order(
    receiver: &[Option<CellId>],
) -> Result<Vec<CellId>, FluvialErosionError> {
    let mut indegree = vec![0_usize; receiver.len()];
    for downstream in receiver.iter().flatten() {
        indegree[downstream.raw() as usize] += 1;
    }
    let mut ready = BinaryHeap::new();
    for (index, &degree) in indegree.iter().enumerate() {
        if degree == 0 {
            ready.push(Reverse(index as u32));
        }
    }
    let mut order = Vec::with_capacity(receiver.len());
    while let Some(Reverse(raw)) = ready.pop() {
        order.push(CellId::from_raw(raw));
        if let Some(downstream) = receiver[raw as usize] {
            let degree = &mut indegree[downstream.raw() as usize];
            *degree -= 1;
            if *degree == 0 {
                ready.push(Reverse(downstream.raw()));
            }
        }
    }
    if order.len() != receiver.len() {
        return Err(FluvialErosionError::ReceiverCycle);
    }
    Ok(order)
}

fn quantize_nearest_centimeter(value_m: f32) -> f32 {
    ((f64::from(value_m) * CENTIMETERS_PER_METER).round() / CENTIMETERS_PER_METER) as f32
}

fn quantize_down_centimeter(value_m: f32) -> f32 {
    ((f64::from(value_m) * CENTIMETERS_PER_METER).floor() / CENTIMETERS_PER_METER) as f32
}

/// Errors returned by the bounded fluvial formation operator.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum FluvialErosionError {
    /// Spatial topology is invalid.
    #[error("invalid spatial input: {0}")]
    Spatial(#[from] SpatialValidationError),
    /// Constructional relief is invalid.
    #[error("invalid relief input: {0}")]
    Relief(#[from] ReliefValidationError),
    /// First-pass hydrology is invalid.
    #[error("invalid first-pass hydrology: {0}")]
    Hydrology(#[from] HydrologyValidationError),
    /// Hydro-erosion controls are invalid.
    #[error("invalid hydro-erosion specification: {0}")]
    Spec(#[from] HydroErosionSpecError),
    /// Generated surface fields are invalid.
    #[error("invalid generated surface process: {0}")]
    Surface(#[from] SurfaceProcessValidationError),
    /// One dense input has a different cardinality.
    #[error("input {input} has length {found}; expected {expected}")]
    CellCountMismatch {
        /// The stable input name.
        input: &'static str,
        /// The required count.
        expected: usize,
        /// The supplied count.
        found: usize,
    },
    /// Erosion resistance is outside its formal domain.
    #[error("erosion resistance {found} at {cell:?} is outside finite 0..=1")]
    ResistanceOutOfRange {
        /// The affected cell.
        cell: CellId,
        /// The rejected value.
        found: f32,
    },
    /// First-pass ocean categories disagree with formal constructional relief.
    #[error("first-pass ocean classification disagrees with relief at {cell:?}")]
    OceanClassificationMismatch {
        /// The affected cell.
        cell: CellId,
    },
    /// The supplied receiver graph unexpectedly contains a cycle.
    #[error("first-pass receiver graph contains a cycle")]
    ReceiverCycle,
}
