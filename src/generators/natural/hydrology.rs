use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use thiserror::Error;

use super::topology::NaturalTopologyIndex;
use crate::engine::BuildCancellation;
use crate::world::natural::{
    formation_monthly_precipitation_mm, BasinOutletKind, ClimateValidationError, DrainageBasin,
    ElevationField, HydroErosionSpec, HydroErosionSpecError, HydrologySnapshot,
    HydrologyValidationError, Lake, LandOceanField, LandOceanKind, PreliminaryClimateSnapshot,
    ReliefValidationError, RiverSegment, RiverSegmentKind, StrahlerOrderField, SurfaceWaterField,
    SurfaceWaterKind, CLIMATE_MONTH_COUNT, ELEVATION_MAX_M, ELEVATION_MIN_M,
    FORMATION_ENDORHEIC_RESIDENCE_YEARS, FORMATION_MINIMUM_LAKE_DEPTH_M,
    FORMATION_RUNOFF_MIN_FRACTION, FORMATION_RUNOFF_PERMEABILITY_RANGE, HYDROLOGY_SCHEMA_V1,
    SECONDS_PER_CLIMATOLOGICAL_MONTH,
};
use crate::world::spatial::{
    NaturalSurface, PlanarNaturalSurface, SpatialSnapshot, SpatialValidationError, Topology,
};
use crate::world::{CellId, DrainageBasinId, LakeId, RiverSegmentId};

const CENTIMETERS_PER_METER: f64 = 100.0;
const METERS_PER_MILLIMETER: f64 = 0.001;
const FORMATION_MINIMUM_LAKE_DEPTH_CM: u16 =
    (FORMATION_MINIMUM_LAKE_DEPTH_M * CENTIMETERS_PER_METER) as u16;
const CANCELLATION_POLL_MASK: usize = 255;

/// Deterministic Priority-Flood, runoff, water-body, basin, and river synthesis.
#[derive(Debug, Clone, Copy, Default)]
pub struct HydrologyGenerator;

impl HydrologyGenerator {
    /// Solves hydrology from only current surface, sea level, permeability, and precipitation.
    pub fn generate(
        spatial: &SpatialSnapshot,
        surface_elevation_m: &ElevationField,
        sea_level_m: f32,
        relative_permeability: &[f32],
        climate: &PreliminaryClimateSnapshot,
        spec: &HydroErosionSpec,
    ) -> Result<HydrologySnapshot, HydrologyGenerationError> {
        spatial.validate()?;
        Self::generate_from_validated_spatial(
            spatial,
            surface_elevation_m,
            sea_level_m,
            relative_permeability,
            climate,
            spec,
        )
    }

    pub(crate) fn generate_from_validated_spatial(
        spatial: &SpatialSnapshot,
        surface_elevation_m: &ElevationField,
        sea_level_m: f32,
        relative_permeability: &[f32],
        climate: &PreliminaryClimateSnapshot,
        spec: &HydroErosionSpec,
    ) -> Result<HydrologySnapshot, HydrologyGenerationError> {
        validate_inputs_against_validated_spatial(
            spatial,
            surface_elevation_m,
            sea_level_m,
            relative_permeability,
            climate,
            spec,
        )?;

        let topology = NaturalTopologyIndex::new(spatial);
        let surface = PlanarNaturalSurface::from_validated(spatial);
        let snapshot = generate_hydrology_core(
            &surface,
            &topology,
            surface_elevation_m,
            sea_level_m,
            relative_permeability,
            climate.monthly_precipitation_mm().values(),
            spec,
            DrainageOutletPolicy::LegacySingleSink,
        )?;
        snapshot.validate_against_validated_spatial(spatial)?;
        Ok(snapshot)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrainageOutletPolicy {
    LegacySingleSink,
    ClosedLocalMinima,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunoffForcingKind {
    LegacyMonthlyTotals,
    FormationMeanDailyRates,
}

#[derive(Debug, Clone, Copy)]
struct HydrologyCoreOptions<'a> {
    outlet_policy: DrainageOutletPolicy,
    runoff_forcing: RunoffForcingKind,
    minimum_lake_depth_cm: u16,
    classify_residence_horizon: bool,
    cancellation: Option<&'a BuildCancellation>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn generate_hydrology_core(
    surface: &impl NaturalSurface,
    topology: &NaturalTopologyIndex,
    surface_elevation_m: &ElevationField,
    sea_level_m: f32,
    relative_permeability: &[f32],
    monthly_precipitation_mm: &[[f32; CLIMATE_MONTH_COUNT]],
    spec: &HydroErosionSpec,
    outlet_policy: DrainageOutletPolicy,
) -> Result<HydrologySnapshot, HydrologyGenerationError> {
    let original_height_cm = quantized_surface_heights(surface_elevation_m, None)?;
    let sea_level_cm = quantize_centimeters_exact(f64::from(sea_level_m));
    let ocean = original_height_cm
        .iter()
        .map(|&height| height < sea_level_cm)
        .collect();
    generate_hydrology_core_impl(
        surface,
        topology,
        original_height_cm,
        ocean,
        relative_permeability,
        monthly_precipitation_mm,
        spec,
        HydrologyCoreOptions {
            outlet_policy,
            runoff_forcing: RunoffForcingKind::LegacyMonthlyTotals,
            minimum_lake_depth_cm: spec.minimum_lake_depth_cm,
            classify_residence_horizon: false,
            cancellation: None,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn generate_formation_hydrology_core(
    surface: &impl NaturalSurface,
    topology: &NaturalTopologyIndex,
    surface_elevation_m: &[f64],
    land_ocean: &LandOceanField,
    relative_permeability: &[f32],
    monthly_precipitation_mm_day: &[[f32; CLIMATE_MONTH_COUNT]],
    spec: &HydroErosionSpec,
    cancellation: &BuildCancellation,
) -> Result<HydrologySnapshot, HydrologyGenerationError> {
    spec.validate()?;
    validate_dense_inputs(
        surface.cell_count(),
        surface_elevation_m,
        relative_permeability,
        monthly_precipitation_mm_day,
        Some(cancellation),
    )?;
    if land_ocean.len() != surface.cell_count() {
        return Err(HydrologyGenerationError::CellCountMismatch {
            input: "land_ocean",
            expected: surface.cell_count(),
            found: land_ocean.len(),
        });
    }
    let original_height_cm =
        quantized_surface_heights_exact(surface_elevation_m, Some(cancellation))?;
    let mut ocean = Vec::with_capacity(land_ocean.len());
    for index in 0..land_ocean.len() {
        poll_cancelled(Some(cancellation), index)?;
        ocean.push(land_ocean.get(index) == Some(LandOceanKind::Ocean));
    }
    generate_hydrology_core_impl(
        surface,
        topology,
        original_height_cm,
        ocean,
        relative_permeability,
        monthly_precipitation_mm_day,
        spec,
        HydrologyCoreOptions {
            outlet_policy: DrainageOutletPolicy::ClosedLocalMinima,
            runoff_forcing: RunoffForcingKind::FormationMeanDailyRates,
            minimum_lake_depth_cm: spec
                .minimum_lake_depth_cm
                .max(FORMATION_MINIMUM_LAKE_DEPTH_CM),
            classify_residence_horizon: true,
            cancellation: Some(cancellation),
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn generate_hydrology_core_impl(
    surface: &impl NaturalSurface,
    topology: &NaturalTopologyIndex,
    original_height_cm: Vec<i64>,
    ocean: Vec<bool>,
    relative_permeability: &[f32],
    monthly_precipitation_mm: &[[f32; CLIMATE_MONTH_COUNT]],
    spec: &HydroErosionSpec,
    options: HydrologyCoreOptions<'_>,
) -> Result<HydrologySnapshot, HydrologyGenerationError> {
    check_cancelled(options.cancellation)?;
    let flood = priority_flood(
        topology,
        &original_height_cm,
        &ocean,
        options.outlet_policy,
        options.cancellation,
    )?;
    let mut receiver = select_receivers(
        topology,
        &original_height_cm,
        &flood.filled_height_cm,
        &flood.rank,
        &flood.terminal,
        options.cancellation,
    )?;
    let mut lakes = identify_and_route_lakes(
        topology,
        &original_height_cm,
        &flood.filled_height_cm,
        &flood.rank,
        &ocean,
        options.minimum_lake_depth_cm,
        &mut receiver,
        options.cancellation,
    )?;
    let order = upstream_to_downstream_order(&receiver, options.cancellation)?;

    let water = build_surface_water(&ocean, &lakes.owner, options.cancellation)?;
    let lake_depth_m = build_lake_depth(
        &original_height_cm,
        &flood.filled_height_cm,
        &lakes.owner,
        options.cancellation,
    )?;
    let monthly_local_runoff_mm = local_runoff(
        &water,
        relative_permeability,
        monthly_precipitation_mm,
        options.runoff_forcing,
        options.cancellation,
    )?;
    if options.classify_residence_horizon {
        close_unfillable_lakes(
            surface,
            &order,
            &monthly_local_runoff_mm,
            &lake_depth_m,
            &mut receiver,
            &mut lakes,
            options.cancellation,
        )?;
    }
    let accumulation = accumulate_water(
        surface,
        &receiver,
        &order,
        &monthly_local_runoff_mm,
        options.cancellation,
    )?;
    let lake_records = build_lake_records(surface, &lakes, &lake_depth_m, options.cancellation)?;
    let (basin_id, basins) = build_basins(
        &receiver,
        &order,
        &water,
        &accumulation.drainage_area_km2,
        &accumulation.mean_discharge_m3_s,
        options.cancellation,
    )?;
    let (strahler_order, river_segments) = build_rivers(
        &receiver,
        &order,
        &water,
        &lakes,
        &accumulation.mean_discharge_m3_s,
        spec.river_discharge_threshold_m3_s(),
        options.cancellation,
    )?;

    let mut drainage_values = Vec::with_capacity(flood.filled_height_cm.len());
    for (index, &height) in flood.filled_height_cm.iter().enumerate() {
        poll_cancelled(options.cancellation, index)?;
        drainage_values.push(height as f32 / CENTIMETERS_PER_METER as f32);
    }
    let drainage_surface_elevation_m = ElevationField::from_values(drainage_values)?;
    let snapshot = HydrologySnapshot::new(
        HYDROLOGY_SCHEMA_V1,
        surface.cell_count() as u32,
        spec.river_discharge_threshold_m3_s(),
        f32::from(options.minimum_lake_depth_cm) / CENTIMETERS_PER_METER as f32,
        monthly_local_runoff_mm,
        accumulation.monthly_discharge_m3_s,
        accumulation.annual_local_runoff_mm,
        accumulation.mean_discharge_m3_s,
        accumulation.drainage_area_km2,
        drainage_surface_elevation_m,
        lake_depth_m,
        SurfaceWaterField::from_kinds(water),
        receiver,
        basin_id,
        StrahlerOrderField::from_raw(strahler_order.into_iter().map(u32::from).collect())?,
        basins,
        lake_records,
        river_segments,
    )?;
    Ok(snapshot)
}

fn validate_dense_inputs(
    cell_count: usize,
    surface_elevation_m: &[f64],
    relative_permeability: &[f32],
    monthly_precipitation_mm_day: &[[f32; CLIMATE_MONTH_COUNT]],
    cancellation: Option<&BuildCancellation>,
) -> Result<(), HydrologyGenerationError> {
    for (input, found) in [
        ("surface_elevation_m", surface_elevation_m.len()),
        ("relative_permeability", relative_permeability.len()),
        (
            "monthly_precipitation_mm_day",
            monthly_precipitation_mm_day.len(),
        ),
    ] {
        if found != cell_count {
            return Err(HydrologyGenerationError::CellCountMismatch {
                input,
                expected: cell_count,
                found,
            });
        }
    }
    for index in 0..cell_count {
        poll_cancelled(cancellation, index)?;
        let elevation = surface_elevation_m[index];
        if !elevation.is_finite()
            || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M)).contains(&elevation)
        {
            return Err(HydrologyGenerationError::SurfaceElevationOutOfRange {
                cell: CellId::from_raw(index as u32),
                found: elevation,
            });
        }
        let permeability = relative_permeability[index];
        if !permeability.is_finite() || !(0.0..=1.0).contains(&permeability) {
            return Err(HydrologyGenerationError::PermeabilityOutOfRange {
                cell: CellId::from_raw(index as u32),
                found: permeability,
            });
        }
        for &precipitation in &monthly_precipitation_mm_day[index] {
            if !precipitation.is_finite() || precipitation < 0.0 {
                return Err(HydrologyGenerationError::PrecipitationRateOutOfRange {
                    cell: CellId::from_raw(index as u32),
                    found: precipitation,
                });
            }
        }
    }
    check_cancelled(cancellation)
}

fn quantized_surface_heights(
    surface_elevation_m: &ElevationField,
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<i64>, HydrologyGenerationError> {
    let mut heights = Vec::with_capacity(surface_elevation_m.len());
    for (index, &value) in surface_elevation_m.values().iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        heights.push(quantize_centimeters_exact(f64::from(value)));
    }
    Ok(heights)
}

fn quantized_surface_heights_exact(
    surface_elevation_m: &[f64],
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<i64>, HydrologyGenerationError> {
    let mut heights = Vec::with_capacity(surface_elevation_m.len());
    for (index, &value) in surface_elevation_m.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        heights.push(quantize_centimeters_exact(value));
    }
    Ok(heights)
}

fn validate_inputs_against_validated_spatial(
    spatial: &SpatialSnapshot,
    surface_elevation_m: &ElevationField,
    sea_level_m: f32,
    relative_permeability: &[f32],
    climate: &PreliminaryClimateSnapshot,
    spec: &HydroErosionSpec,
) -> Result<(), HydrologyGenerationError> {
    climate.validate()?;
    spec.validate()?;
    let cell_count = spatial.cell_count();
    if surface_elevation_m.len() != cell_count {
        return Err(HydrologyGenerationError::CellCountMismatch {
            input: "surface_elevation_m",
            expected: cell_count,
            found: surface_elevation_m.len(),
        });
    }
    if relative_permeability.len() != cell_count {
        return Err(HydrologyGenerationError::CellCountMismatch {
            input: "relative_permeability",
            expected: cell_count,
            found: relative_permeability.len(),
        });
    }
    if climate.cell_count() as usize != cell_count {
        return Err(HydrologyGenerationError::CellCountMismatch {
            input: "preliminary_climate",
            expected: cell_count,
            found: climate.cell_count() as usize,
        });
    }
    if !sea_level_m.is_finite() {
        return Err(HydrologyGenerationError::NonFiniteSeaLevel { found: sea_level_m });
    }
    for (index, &found) in surface_elevation_m.values().iter().enumerate() {
        if !found.is_finite() || !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&found) {
            return Err(HydrologyGenerationError::SurfaceElevationOutOfRange {
                cell: CellId::from_raw(index as u32),
                found: f64::from(found),
            });
        }
    }
    for (index, &found) in relative_permeability.iter().enumerate() {
        if !found.is_finite() || !(0.0..=1.0).contains(&found) {
            return Err(HydrologyGenerationError::PermeabilityOutOfRange {
                cell: CellId::from_raw(index as u32),
                found,
            });
        }
    }
    Ok(())
}

struct FloodResult {
    filled_height_cm: Vec<i64>,
    rank: Vec<u32>,
    terminal: Vec<bool>,
}

fn priority_flood(
    topology: &NaturalTopologyIndex,
    original_height_cm: &[i64],
    ocean: &[bool],
    outlet_policy: DrainageOutletPolicy,
    cancellation: Option<&BuildCancellation>,
) -> Result<FloodResult, HydrologyGenerationError> {
    let cell_count = original_height_cm.len();
    let mut terminal = ocean.to_vec();
    if !terminal.iter().any(|&value| value) {
        match outlet_policy {
            DrainageOutletPolicy::LegacySingleSink => {
                let sink = (0..cell_count)
                    .min_by_key(|&index| (original_height_cm[index], index))
                    .ok_or(HydrologyGenerationError::EmptyWorld)?;
                terminal[sink] = true;
            }
            DrainageOutletPolicy::ClosedLocalMinima => {
                terminal =
                    closed_local_minimum_terminals(topology, original_height_cm, cancellation)?;
            }
        }
    }

    let mut filled_height_cm = vec![i64::MAX; cell_count];
    let mut rank = vec![u32::MAX; cell_count];
    let mut visited = vec![false; cell_count];
    let mut heap = BinaryHeap::new();
    for index in 0..cell_count {
        if terminal[index] {
            visited[index] = true;
            filled_height_cm[index] = original_height_cm[index];
            heap.push(Reverse((original_height_cm[index], index as u32)));
        }
    }

    let mut next_rank = 0_u32;
    while let Some(Reverse((height, raw_cell))) = heap.pop() {
        poll_cancelled(cancellation, next_rank as usize)?;
        let index = raw_cell as usize;
        rank[index] = next_rank;
        next_rank = next_rank
            .checked_add(1)
            .ok_or(HydrologyGenerationError::DrainageRankOverflow)?;
        for arc in &topology.arcs()[index] {
            let neighbor = arc.neighbor.raw() as usize;
            if visited[neighbor] {
                continue;
            }
            visited[neighbor] = true;
            let filled = original_height_cm[neighbor].max(height);
            filled_height_cm[neighbor] = filled;
            heap.push(Reverse((filled, arc.neighbor.raw())));
        }
    }
    if rank.contains(&u32::MAX) {
        return Err(HydrologyGenerationError::DisconnectedTopology);
    }
    Ok(FloodResult {
        filled_height_cm,
        rank,
        terminal,
    })
}

fn closed_local_minimum_terminals(
    topology: &NaturalTopologyIndex,
    original_height_cm: &[i64],
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<bool>, HydrologyGenerationError> {
    if original_height_cm.is_empty() {
        return Err(HydrologyGenerationError::EmptyWorld);
    }
    let mut terminal = vec![false; original_height_cm.len()];
    let mut visited = vec![false; original_height_cm.len()];
    for start in 0..original_height_cm.len() {
        poll_cancelled(cancellation, start)?;
        if visited[start] {
            continue;
        }
        let plateau_height = original_height_cm[start];
        let mut representative = CellId::from_raw(start as u32);
        let mut is_local_minimum = true;
        let mut queue = VecDeque::from([representative]);
        visited[start] = true;
        while let Some(cell) = queue.pop_front() {
            representative = representative.min(cell);
            for arc in &topology.arcs()[cell.raw() as usize] {
                let neighbor_index = arc.neighbor.raw() as usize;
                let neighbor_height = original_height_cm[neighbor_index];
                if neighbor_height < plateau_height {
                    is_local_minimum = false;
                } else if neighbor_height == plateau_height && !visited[neighbor_index] {
                    visited[neighbor_index] = true;
                    queue.push_back(arc.neighbor);
                }
            }
        }
        if is_local_minimum {
            terminal[representative.raw() as usize] = true;
        }
    }
    if !terminal.iter().any(|&value| value) {
        return Err(HydrologyGenerationError::MissingClosedTerminal);
    }
    Ok(terminal)
}

fn select_receivers(
    topology: &NaturalTopologyIndex,
    original_height_cm: &[i64],
    filled_height_cm: &[i64],
    rank: &[u32],
    terminal: &[bool],
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<Option<CellId>>, HydrologyGenerationError> {
    let mut receivers = vec![None; original_height_cm.len()];
    for index in 0..original_height_cm.len() {
        poll_cancelled(cancellation, index)?;
        if terminal[index] {
            continue;
        }
        let mut steepest: Option<(CellId, i64, u64)> = None;
        let mut fallback: Option<CellId> = None;
        for arc in &topology.arcs()[index] {
            let neighbor_index = arc.neighbor.raw() as usize;
            let drains = filled_height_cm[neighbor_index] < filled_height_cm[index]
                || (filled_height_cm[neighbor_index] == filled_height_cm[index]
                    && rank[neighbor_index] < rank[index]);
            if !drains {
                continue;
            }
            if fallback.is_none_or(|best| {
                drainage_key(arc.neighbor, filled_height_cm, rank)
                    < drainage_key(best, filled_height_cm, rank)
            }) {
                fallback = Some(arc.neighbor);
            }

            let drop_cm = original_height_cm[index] - original_height_cm[neighbor_index];
            if drop_cm <= 0 {
                continue;
            }
            let candidate = (arc.neighbor, drop_cm, arc.traversal_cost);
            if steepest.is_none_or(|best| is_steeper(candidate, best, filled_height_cm, rank)) {
                steepest = Some(candidate);
            }
        }
        receivers[index] = steepest
            .map(|candidate| candidate.0)
            .or(fallback)
            .ok_or(HydrologyGenerationError::MissingReceiver {
                cell: CellId::from_raw(index as u32),
            })?
            .into();
    }
    Ok(receivers)
}

fn drainage_key(cell: CellId, filled_height_cm: &[i64], rank: &[u32]) -> (i64, u32, CellId) {
    let index = cell.raw() as usize;
    (filled_height_cm[index], rank[index], cell)
}

fn is_steeper(
    candidate: (CellId, i64, u64),
    incumbent: (CellId, i64, u64),
    filled_height_cm: &[i64],
    rank: &[u32],
) -> bool {
    let candidate_cross = i128::from(candidate.1) * i128::from(incumbent.2);
    let incumbent_cross = i128::from(incumbent.1) * i128::from(candidate.2);
    candidate_cross > incumbent_cross
        || (candidate_cross == incumbent_cross
            && drainage_key(candidate.0, filled_height_cm, rank)
                < drainage_key(incumbent.0, filled_height_cm, rank))
}

struct LakeDraft {
    cells: Vec<CellId>,
    surface_elevation_m: f32,
    outlet_cell: Option<CellId>,
    downstream_cell: Option<CellId>,
}

struct LakeRouting {
    owner: Vec<Option<usize>>,
    drafts: Vec<LakeDraft>,
}

#[allow(clippy::too_many_arguments)]
fn identify_and_route_lakes(
    topology: &NaturalTopologyIndex,
    original_height_cm: &[i64],
    filled_height_cm: &[i64],
    rank: &[u32],
    ocean: &[bool],
    minimum_lake_depth_cm: u16,
    receiver: &mut [Option<CellId>],
    cancellation: Option<&BuildCancellation>,
) -> Result<LakeRouting, HydrologyGenerationError> {
    let cell_count = original_height_cm.len();
    let minimum_depth = i64::from(minimum_lake_depth_cm);
    let candidate = (0..cell_count)
        .map(|index| {
            !ocean[index] && filled_height_cm[index] - original_height_cm[index] >= minimum_depth
        })
        .collect::<Vec<_>>();
    let mut owner = vec![None; cell_count];
    let mut components = Vec::new();

    for start in 0..cell_count {
        poll_cancelled(cancellation, start)?;
        if !candidate[start] || owner[start].is_some() {
            continue;
        }
        let component = components.len();
        let lake_level = filled_height_cm[start];
        let mut cells = Vec::new();
        let mut queue = VecDeque::from([CellId::from_raw(start as u32)]);
        owner[start] = Some(component);
        while let Some(cell) = queue.pop_front() {
            cells.push(cell);
            for arc in &topology.arcs()[cell.raw() as usize] {
                let neighbor = arc.neighbor.raw() as usize;
                if candidate[neighbor]
                    && owner[neighbor].is_none()
                    && filled_height_cm[neighbor] == lake_level
                {
                    owner[neighbor] = Some(component);
                    queue.push_back(arc.neighbor);
                }
            }
        }
        cells.sort();
        components.push(cells);
    }

    let mut drafts = Vec::with_capacity(components.len());
    for (component, cells) in components.into_iter().enumerate() {
        poll_cancelled(cancellation, component)?;
        let mut outlet_pair: Option<(CellId, CellId)> = None;
        let mut closed_terminal = None;
        for &cell in &cells {
            match receiver[cell.raw() as usize] {
                Some(downstream) if owner[downstream.raw() as usize] != Some(component) => {
                    let candidate_pair = (cell, downstream);
                    if outlet_pair.is_none_or(|incumbent| {
                        lake_outlet_key(candidate_pair, filled_height_cm, rank)
                            < lake_outlet_key(incumbent, filled_height_cm, rank)
                    }) {
                        outlet_pair = Some(candidate_pair);
                    }
                }
                None => {
                    closed_terminal =
                        Some(closed_terminal.map_or(cell, |incumbent: CellId| incumbent.min(cell)));
                }
                _ => {}
            }
        }

        let routing_root;
        let (outlet_cell, downstream_cell) = if let Some((outlet, downstream)) = outlet_pair {
            receiver[outlet.raw() as usize] = Some(downstream);
            routing_root = outlet;
            (Some(outlet), Some(downstream))
        } else if let Some(terminal) = closed_terminal {
            receiver[terminal.raw() as usize] = None;
            routing_root = terminal;
            (None, None)
        } else {
            return Err(HydrologyGenerationError::MissingLakeOutlet { cell: cells[0] });
        };

        let mut routed = vec![false; cell_count];
        let mut queue = VecDeque::from([routing_root]);
        routed[routing_root.raw() as usize] = true;
        while let Some(cell) = queue.pop_front() {
            for arc in &topology.arcs()[cell.raw() as usize] {
                let neighbor = arc.neighbor.raw() as usize;
                if owner[neighbor] == Some(component) && !routed[neighbor] {
                    routed[neighbor] = true;
                    receiver[neighbor] = Some(cell);
                    queue.push_back(arc.neighbor);
                }
            }
        }
        if cells.iter().any(|cell| !routed[cell.raw() as usize]) {
            return Err(HydrologyGenerationError::DisconnectedLake { cell: cells[0] });
        }

        let surface_elevation_m =
            filled_height_cm[cells[0].raw() as usize] as f32 / CENTIMETERS_PER_METER as f32;
        drafts.push(LakeDraft {
            cells,
            surface_elevation_m,
            outlet_cell,
            downstream_cell,
        });
    }

    for (index, cell_owner) in owner.iter().enumerate() {
        if let Some(component) = cell_owner {
            debug_assert!(
                drafts[*component]
                    .cells
                    .binary_search(&CellId::from_raw(index as u32))
                    .is_ok(),
                "lake ownership and sorted membership remain aligned"
            );
        }
    }
    Ok(LakeRouting { owner, drafts })
}

fn lake_outlet_key(
    pair: (CellId, CellId),
    filled_height_cm: &[i64],
    rank: &[u32],
) -> (i64, u32, CellId, CellId) {
    let downstream = pair.1.raw() as usize;
    (
        filled_height_cm[downstream],
        rank[downstream],
        pair.1,
        pair.0,
    )
}

fn upstream_to_downstream_order(
    receiver: &[Option<CellId>],
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<CellId>, HydrologyGenerationError> {
    let mut indegree = vec![0_usize; receiver.len()];
    for (index, downstream) in receiver.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        if let Some(downstream) = downstream {
            indegree[downstream.raw() as usize] += 1;
        }
    }
    let mut ready = BinaryHeap::new();
    for (index, &degree) in indegree.iter().enumerate() {
        if degree == 0 {
            ready.push(Reverse(index as u32));
        }
    }
    let mut order = Vec::with_capacity(receiver.len());
    while let Some(Reverse(raw_cell)) = ready.pop() {
        poll_cancelled(cancellation, order.len())?;
        let cell = CellId::from_raw(raw_cell);
        order.push(cell);
        if let Some(downstream) = receiver[raw_cell as usize] {
            let degree = &mut indegree[downstream.raw() as usize];
            *degree -= 1;
            if *degree == 0 {
                ready.push(Reverse(downstream.raw()));
            }
        }
    }
    if order.len() != receiver.len() {
        return Err(HydrologyGenerationError::ReceiverCycle);
    }
    Ok(order)
}

fn build_surface_water(
    ocean: &[bool],
    lake_owner: &[Option<usize>],
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<SurfaceWaterKind>, HydrologyGenerationError> {
    let mut water = Vec::with_capacity(ocean.len());
    for (index, (&is_ocean, lake)) in ocean.iter().zip(lake_owner).enumerate() {
        poll_cancelled(cancellation, index)?;
        water.push(if is_ocean {
            SurfaceWaterKind::Ocean
        } else if lake.is_some() {
            SurfaceWaterKind::Lake
        } else {
            SurfaceWaterKind::DryLand
        });
    }
    Ok(water)
}

fn build_lake_depth(
    original_height_cm: &[i64],
    filled_height_cm: &[i64],
    lake_owner: &[Option<usize>],
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<f32>, HydrologyGenerationError> {
    let mut depth = Vec::with_capacity(original_height_cm.len());
    for index in 0..original_height_cm.len() {
        poll_cancelled(cancellation, index)?;
        depth.push(if lake_owner[index].is_some() {
            (filled_height_cm[index] - original_height_cm[index]) as f32
                / CENTIMETERS_PER_METER as f32
        } else {
            0.0
        });
    }
    Ok(depth)
}

fn local_runoff(
    water: &[SurfaceWaterKind],
    relative_permeability: &[f32],
    monthly_precipitation_mm: &[[f32; CLIMATE_MONTH_COUNT]],
    forcing_kind: RunoffForcingKind,
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<[f32; CLIMATE_MONTH_COUNT]>, HydrologyGenerationError> {
    let mut runoff = Vec::with_capacity(water.len());
    for index in 0..water.len() {
        poll_cancelled(cancellation, index)?;
        let months = if water[index] == SurfaceWaterKind::Ocean {
            [0.0; CLIMATE_MONTH_COUNT]
        } else {
            match forcing_kind {
                RunoffForcingKind::LegacyMonthlyTotals => {
                    let runoff_fraction = 0.85 + (0.20 - 0.85) * relative_permeability[index];
                    std::array::from_fn(|month| {
                        monthly_precipitation_mm[index][month] * runoff_fraction
                    })
                }
                RunoffForcingKind::FormationMeanDailyRates => {
                    let runoff_fraction = FORMATION_RUNOFF_MIN_FRACTION
                        + FORMATION_RUNOFF_PERMEABILITY_RANGE
                            * (1.0 - f64::from(relative_permeability[index]));
                    let bounded =
                        formation_monthly_precipitation_mm(&monthly_precipitation_mm[index]);
                    std::array::from_fn(|month| (bounded[month] * runoff_fraction) as f32)
                }
            }
        };
        runoff.push(months);
    }
    check_cancelled(cancellation)?;
    Ok(runoff)
}

fn close_unfillable_lakes(
    surface: &impl NaturalSurface,
    order: &[CellId],
    monthly_local_runoff_mm: &[[f32; CLIMATE_MONTH_COUNT]],
    lake_depth_m: &[f32],
    receiver: &mut [Option<CellId>],
    lakes: &mut LakeRouting,
    cancellation: Option<&BuildCancellation>,
) -> Result<(), HydrologyGenerationError> {
    let mut outlet_component = vec![None; receiver.len()];
    let mut spill_volume_m3 = vec![0.0_f64; lakes.drafts.len()];
    for (component, draft) in lakes.drafts.iter().enumerate() {
        poll_cancelled(cancellation, component)?;
        if let Some(outlet) = draft.outlet_cell {
            outlet_component[outlet.raw() as usize] = Some(component);
        }
        for &cell in &draft.cells {
            let index = cell.raw() as usize;
            let area_m2 = surface
                .cell(cell)
                .expect("validated lake cell exists on the natural surface")
                .area()
                .get();
            spill_volume_m3[component] += area_m2 * f64::from(lake_depth_m[index]);
        }
    }

    let mut annual_catchment_runoff_m3 = vec![0.0_f64; receiver.len()];
    for index in 0..receiver.len() {
        poll_cancelled(cancellation, index)?;
        let area_m2 = surface
            .cell(CellId::from_raw(index as u32))
            .expect("validated dense cell exists on the natural surface")
            .area()
            .get();
        annual_catchment_runoff_m3[index] = monthly_local_runoff_mm[index]
            .iter()
            .map(|&runoff_mm| f64::from(runoff_mm) * METERS_PER_MILLIMETER * area_m2)
            .sum();
    }

    for (position, &cell) in order.iter().enumerate() {
        poll_cancelled(cancellation, position)?;
        let index = cell.raw() as usize;
        if let Some(component) = outlet_component[index] {
            let fillable_volume_m3 =
                annual_catchment_runoff_m3[index] * FORMATION_ENDORHEIC_RESIDENCE_YEARS;
            if fillable_volume_m3 < spill_volume_m3[component] {
                receiver[index] = None;
                lakes.drafts[component].outlet_cell = None;
                lakes.drafts[component].downstream_cell = None;
            }
        }
        if let Some(downstream) = receiver[index] {
            annual_catchment_runoff_m3[downstream.raw() as usize] +=
                annual_catchment_runoff_m3[index];
        }
    }
    check_cancelled(cancellation)
}

struct WaterAccumulation {
    monthly_discharge_m3_s: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    annual_local_runoff_mm: Vec<f32>,
    mean_discharge_m3_s: Vec<f32>,
    drainage_area_km2: Vec<f32>,
}

fn accumulate_water(
    surface: &impl NaturalSurface,
    receiver: &[Option<CellId>],
    order: &[CellId],
    monthly_local_runoff_mm: &[[f32; CLIMATE_MONTH_COUNT]],
    cancellation: Option<&BuildCancellation>,
) -> Result<WaterAccumulation, HydrologyGenerationError> {
    let cell_count = receiver.len();
    let mut area_km2 = vec![0.0_f64; cell_count];
    let mut discharge = vec![[0.0_f64; CLIMATE_MONTH_COUNT]; cell_count];
    for index in 0..cell_count {
        poll_cancelled(cancellation, index)?;
        let area_m2 = surface
            .cell(CellId::from_raw(index as u32))
            .expect("validated natural surface contains every dense cell")
            .area()
            .get();
        area_km2[index] = area_m2 / 1_000_000.0;
        for (stored, &runoff_mm) in discharge[index]
            .iter_mut()
            .zip(&monthly_local_runoff_mm[index])
        {
            *stored = f64::from(runoff_mm) / 1_000.0 * area_m2 / SECONDS_PER_CLIMATOLOGICAL_MONTH;
        }
    }
    for (position, &cell) in order.iter().enumerate() {
        poll_cancelled(cancellation, position)?;
        let index = cell.raw() as usize;
        if let Some(downstream) = receiver[index] {
            let downstream_index = downstream.raw() as usize;
            let upstream_area = area_km2[index];
            area_km2[downstream_index] += upstream_area;
            let upstream_discharge = discharge[index];
            for (downstream, upstream) in discharge[downstream_index]
                .iter_mut()
                .zip(upstream_discharge)
            {
                *downstream += upstream;
            }
        }
    }

    let monthly_discharge_m3_s = discharge
        .iter()
        .map(|months| months.map(|value| value as f32))
        .collect();
    let annual_local_runoff_mm = monthly_local_runoff_mm
        .iter()
        .map(|months| months.iter().map(|&value| f64::from(value)).sum::<f64>() as f32)
        .collect();
    let mean_discharge_m3_s = discharge
        .iter()
        .map(|months| (months.iter().sum::<f64>() / CLIMATE_MONTH_COUNT as f64) as f32)
        .collect();
    let drainage_area_km2 = area_km2.into_iter().map(|value| value as f32).collect();
    Ok(WaterAccumulation {
        monthly_discharge_m3_s,
        annual_local_runoff_mm,
        mean_discharge_m3_s,
        drainage_area_km2,
    })
}

fn build_lake_records(
    surface: &impl NaturalSurface,
    lakes: &LakeRouting,
    lake_depth_m: &[f32],
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<Lake>, HydrologyGenerationError> {
    let mut records = Vec::with_capacity(lakes.drafts.len());
    for (index, draft) in lakes.drafts.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        let mut area_m2 = 0.0;
        let mut volume_m3 = 0.0;
        for &cell in &draft.cells {
            let area = surface
                .cell(cell)
                .expect("validated lake cells exist in the natural surface")
                .area()
                .get();
            area_m2 += area;
            volume_m3 += area * f64::from(lake_depth_m[cell.raw() as usize]);
        }
        records.push(Lake::new(
            LakeId::from_raw(index as u32),
            draft.cells.clone(),
            draft.surface_elevation_m,
            area_m2 / 1_000_000.0,
            volume_m3,
            draft.outlet_cell,
            draft.downstream_cell,
        )?);
    }
    Ok(records)
}

fn build_basins(
    receiver: &[Option<CellId>],
    order: &[CellId],
    water: &[SurfaceWaterKind],
    drainage_area_km2: &[f32],
    mean_discharge_m3_s: &[f32],
    cancellation: Option<&BuildCancellation>,
) -> Result<(Vec<Option<DrainageBasinId>>, Vec<DrainageBasin>), HydrologyGenerationError> {
    let mut root = vec![None; receiver.len()];
    for (position, &cell) in order.iter().rev().enumerate() {
        poll_cancelled(cancellation, position)?;
        let index = cell.raw() as usize;
        root[index] = Some(match receiver[index] {
            Some(downstream) => root[downstream.raw() as usize]
                .expect("reverse topological order resolves downstream root first"),
            None => cell,
        });
    }
    let mut used_terminal = vec![false; receiver.len()];
    for index in 0..receiver.len() {
        if water[index] != SurfaceWaterKind::Ocean {
            used_terminal[root[index].expect("every cell has a root").raw() as usize] = true;
        }
    }

    let mut terminal_to_basin = vec![None; receiver.len()];
    let mut basins = Vec::new();
    for index in 0..receiver.len() {
        poll_cancelled(cancellation, index)?;
        if !used_terminal[index] {
            continue;
        }
        let id = DrainageBasinId::from_raw(basins.len() as u32);
        terminal_to_basin[index] = Some(id);
        let outlet = CellId::from_raw(index as u32);
        let outlet_kind = match water[index] {
            SurfaceWaterKind::Ocean => BasinOutletKind::Ocean,
            SurfaceWaterKind::Lake => BasinOutletKind::Lake,
            SurfaceWaterKind::DryLand => BasinOutletKind::ClosedSink,
        };
        basins.push(DrainageBasin::new(
            id,
            outlet,
            outlet_kind,
            f64::from(drainage_area_km2[index]),
            mean_discharge_m3_s[index],
        )?);
    }
    let basin_id = (0..receiver.len())
        .map(|index| {
            if water[index] == SurfaceWaterKind::Ocean {
                None
            } else {
                let terminal = root[index].expect("every cell has a terminal").raw() as usize;
                terminal_to_basin[terminal]
            }
        })
        .collect();
    Ok((basin_id, basins))
}

fn build_rivers(
    receiver: &[Option<CellId>],
    order: &[CellId],
    water: &[SurfaceWaterKind],
    lakes: &LakeRouting,
    mean_discharge_m3_s: &[f32],
    threshold_m3_s: f32,
    cancellation: Option<&BuildCancellation>,
) -> Result<(Vec<u8>, Vec<RiverSegment>), HydrologyGenerationError> {
    let mut lake_outlet = vec![false; receiver.len()];
    for draft in &lakes.drafts {
        if let Some(outlet) = draft.outlet_cell {
            lake_outlet[outlet.raw() as usize] = true;
        }
    }
    let eligible = (0..receiver.len())
        .map(|index| {
            receiver[index].is_some()
                && mean_discharge_m3_s[index] >= threshold_m3_s
                && (water[index] == SurfaceWaterKind::DryLand || lake_outlet[index])
        })
        .collect::<Vec<_>>();

    let mut cell_max = vec![0_u8; receiver.len()];
    let mut cell_max_count = vec![0_u32; receiver.len()];
    let mut lake_max = vec![0_u8; lakes.drafts.len()];
    let mut lake_max_count = vec![0_u32; lakes.drafts.len()];
    let mut order_field = vec![0_u8; receiver.len()];

    for (position, &cell) in order.iter().enumerate() {
        poll_cancelled(cancellation, position)?;
        let index = cell.raw() as usize;
        if !eligible[index] {
            continue;
        }
        let (incoming_max, incoming_count) = if lake_outlet[index] {
            let lake = lakes.owner[index].expect("lake outlet belongs to a lake");
            (lake_max[lake], lake_max_count[lake])
        } else {
            (cell_max[index], cell_max_count[index])
        };
        let stream_order = if incoming_max == 0 {
            1
        } else if incoming_count >= 2 {
            incoming_max
                .checked_add(1)
                .ok_or(HydrologyGenerationError::StrahlerOverflow { cell })?
        } else {
            incoming_max
        };
        order_field[index] = stream_order;

        if let Some(downstream) = receiver[index] {
            let downstream_index = downstream.raw() as usize;
            if let Some(lake) = lakes.owner[downstream_index] {
                update_strahler_aggregate(
                    &mut lake_max[lake],
                    &mut lake_max_count[lake],
                    stream_order,
                );
            } else {
                update_strahler_aggregate(
                    &mut cell_max[downstream_index],
                    &mut cell_max_count[downstream_index],
                    stream_order,
                );
            }
        }
    }

    let mut segments = Vec::new();
    for index in 0..receiver.len() {
        poll_cancelled(cancellation, index)?;
        if !eligible[index] {
            continue;
        }
        let from = CellId::from_raw(index as u32);
        let to = receiver[index].expect("eligible segment origin has a receiver");
        let kind = if lake_outlet[index] {
            RiverSegmentKind::LakeOutlet
        } else {
            RiverSegmentKind::Channel
        };
        segments.push(RiverSegment::new(
            RiverSegmentId::from_raw(segments.len() as u32),
            from,
            to,
            kind,
            order_field[index],
            mean_discharge_m3_s[index],
        )?);
    }
    Ok((order_field, segments))
}

fn update_strahler_aggregate(maximum: &mut u8, count: &mut u32, order: u8) {
    if order > *maximum {
        *maximum = order;
        *count = 1;
    } else if order == *maximum {
        *count += 1;
    }
}

fn quantize_centimeters_exact(value_m: f64) -> i64 {
    (value_m * CENTIMETERS_PER_METER).round() as i64
}

fn poll_cancelled(
    cancellation: Option<&BuildCancellation>,
    index: usize,
) -> Result<(), HydrologyGenerationError> {
    if index & CANCELLATION_POLL_MASK == 0 {
        check_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_cancelled(
    cancellation: Option<&BuildCancellation>,
) -> Result<(), HydrologyGenerationError> {
    if cancellation.is_some_and(BuildCancellation::is_cancelled) {
        Err(HydrologyGenerationError::Cancelled)
    } else {
        Ok(())
    }
}

/// Errors returned by the pure deterministic hydrology solver.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum HydrologyGenerationError {
    /// Cooperative cancellation interrupted active hydrology work.
    #[error("hydrology generation cancelled")]
    Cancelled,
    /// The spatial topology is invalid.
    #[error("invalid spatial input: {0}")]
    Spatial(#[from] SpatialValidationError),
    /// The hydro-erosion controls are invalid.
    #[error("invalid hydro-erosion specification: {0}")]
    Spec(#[from] HydroErosionSpecError),
    /// The preliminary climate forcing is invalid.
    #[error("invalid preliminary climate input: {0}")]
    Climate(#[from] ClimateValidationError),
    /// A generated elevation field is invalid.
    #[error("invalid generated elevation field: {0}")]
    Relief(#[from] ReliefValidationError),
    /// The generated formal hydrology snapshot is invalid.
    #[error("invalid generated hydrology: {0}")]
    Hydrology(#[from] HydrologyValidationError),
    /// One dense input has a different cardinality.
    #[error("input {input} has length {found}; expected {expected}")]
    CellCountMismatch {
        /// The stable input name.
        input: &'static str,
        /// The spatial cell count.
        expected: usize,
        /// The supplied count.
        found: usize,
    },
    /// Sea level is non-finite.
    #[error("sea level must be finite, got {found}")]
    NonFiniteSeaLevel {
        /// The rejected sea level.
        found: f32,
    },
    /// Current surface elevation is invalid.
    #[error("surface elevation {found} at {cell:?} is outside the supported finite range")]
    SurfaceElevationOutOfRange {
        /// The affected cell.
        cell: CellId,
        /// The rejected elevation.
        found: f64,
    },
    /// Relative permeability is invalid.
    #[error("relative permeability {found} at {cell:?} is outside finite 0..=1")]
    PermeabilityOutOfRange {
        /// The affected cell.
        cell: CellId,
        /// The rejected permeability.
        found: f32,
    },
    /// A formation-climate precipitation rate is invalid.
    #[error("precipitation rate {found} mm/day at {cell:?} is outside finite nonnegative values")]
    PrecipitationRateOutOfRange {
        /// The affected cell.
        cell: CellId,
        /// The rejected precipitation rate.
        found: f32,
    },
    /// No cells were supplied.
    #[error("hydrology cannot solve an empty world")]
    EmptyWorld,
    /// The stable drainage rank exceeded its representation.
    #[error("hydrology drainage rank exceeded u32")]
    DrainageRankOverflow,
    /// The validated topology was unexpectedly disconnected.
    #[error("priority flood could not reach every spatial cell")]
    DisconnectedTopology,
    /// A nonempty closed surface unexpectedly had no local-minimum plateau.
    #[error("closed hydrology could not identify an endorheic terminal")]
    MissingClosedTerminal,
    /// A nonterminal cell had no legal earlier drainage neighbor.
    #[error("cell {cell:?} has no legal drainage receiver")]
    MissingReceiver {
        /// The affected cell.
        cell: CellId,
    },
    /// A published lake component had neither an outflow nor a terminal.
    #[error("lake component containing {cell:?} has no stable outlet")]
    MissingLakeOutlet {
        /// One member cell.
        cell: CellId,
    },
    /// A lake component was not spatially connected during canonical routing.
    #[error("lake component containing {cell:?} is disconnected")]
    DisconnectedLake {
        /// One member cell.
        cell: CellId,
    },
    /// Canonical lake routing unexpectedly formed a receiver cycle.
    #[error("canonical receiver graph contains a cycle")]
    ReceiverCycle,
    /// A branching river network exceeded the V1 order bound.
    #[error("Strahler order overflow at {cell:?}")]
    StrahlerOverflow {
        /// The affected river cell.
        cell: CellId,
    },
}
