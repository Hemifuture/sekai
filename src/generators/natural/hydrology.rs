use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use thiserror::Error;

use super::topology::NaturalTopologyIndex;
use crate::world::natural::{
    BasinOutletKind, ClimateValidationError, DrainageBasin, ElevationField, HydroErosionSpec,
    HydroErosionSpecError, HydrologySnapshot, HydrologyValidationError, Lake,
    PreliminaryClimateSnapshot, ReliefValidationError, RiverSegment, RiverSegmentKind,
    StrahlerOrderField, SurfaceWaterField, SurfaceWaterKind, CLIMATE_MONTH_COUNT, ELEVATION_MAX_M,
    ELEVATION_MIN_M, HYDROLOGY_SCHEMA_V1, SECONDS_PER_CLIMATOLOGICAL_MONTH,
};
use crate::world::spatial::{SpatialSnapshot, SpatialValidationError, Topology};
use crate::world::{CellId, DrainageBasinId, LakeId, RiverSegmentId};

const CENTIMETERS_PER_METER: f64 = 100.0;

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
        let original_height_cm = surface_elevation_m
            .values()
            .iter()
            .map(|&value| quantize_centimeters(value))
            .collect::<Vec<_>>();
        let sea_level_cm = quantize_centimeters(sea_level_m);
        let ocean = original_height_cm
            .iter()
            .map(|&height| height < sea_level_cm)
            .collect::<Vec<_>>();

        let flood = priority_flood(&topology, &original_height_cm, &ocean)?;
        let mut receiver = select_receivers(
            &topology,
            &original_height_cm,
            &flood.filled_height_cm,
            &flood.rank,
            &flood.terminal,
        )?;
        let lakes = identify_and_route_lakes(
            &topology,
            &original_height_cm,
            &flood.filled_height_cm,
            &flood.rank,
            &ocean,
            spec.minimum_lake_depth_cm,
            &mut receiver,
        )?;
        let order = upstream_to_downstream_order(&receiver)?;

        let water = build_surface_water(&ocean, &lakes.owner);
        let lake_depth_m =
            build_lake_depth(&original_height_cm, &flood.filled_height_cm, &lakes.owner);
        let monthly_local_runoff_mm = local_runoff(&water, relative_permeability, climate);
        let accumulation = accumulate_water(spatial, &receiver, &order, &monthly_local_runoff_mm);
        let lake_records = build_lake_records(spatial, &lakes, &lake_depth_m)?;
        let (basin_id, basins) = build_basins(
            &receiver,
            &order,
            &water,
            &accumulation.drainage_area_km2,
            &accumulation.mean_discharge_m3_s,
        )?;
        let (strahler_order, river_segments) = build_rivers(
            &receiver,
            &order,
            &water,
            &lakes,
            &accumulation.mean_discharge_m3_s,
            spec.river_discharge_threshold_m3_s(),
        )?;

        let drainage_surface_elevation_m = ElevationField::from_values(
            flood
                .filled_height_cm
                .iter()
                .map(|&height| height as f32 / CENTIMETERS_PER_METER as f32)
                .collect(),
        )?;
        let snapshot = HydrologySnapshot::new(
            HYDROLOGY_SCHEMA_V1,
            spatial.cell_count() as u32,
            spec.river_discharge_threshold_m3_s(),
            spec.minimum_lake_depth_m(),
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
        snapshot.validate_against_validated_spatial(spatial)?;
        Ok(snapshot)
    }
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
                found,
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
) -> Result<FloodResult, HydrologyGenerationError> {
    let cell_count = original_height_cm.len();
    let mut terminal = ocean.to_vec();
    if !terminal.iter().any(|&value| value) {
        let sink = (0..cell_count)
            .min_by_key(|&index| (original_height_cm[index], index))
            .ok_or(HydrologyGenerationError::EmptyWorld)?;
        terminal[sink] = true;
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

fn select_receivers(
    topology: &NaturalTopologyIndex,
    original_height_cm: &[i64],
    filled_height_cm: &[i64],
    rank: &[u32],
    terminal: &[bool],
) -> Result<Vec<Option<CellId>>, HydrologyGenerationError> {
    let mut receivers = vec![None; original_height_cm.len()];
    for index in 0..original_height_cm.len() {
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
) -> Result<Vec<CellId>, HydrologyGenerationError> {
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
    while let Some(Reverse(raw_cell)) = ready.pop() {
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

fn build_surface_water(ocean: &[bool], lake_owner: &[Option<usize>]) -> Vec<SurfaceWaterKind> {
    ocean
        .iter()
        .zip(lake_owner)
        .map(|(&is_ocean, lake)| {
            if is_ocean {
                SurfaceWaterKind::Ocean
            } else if lake.is_some() {
                SurfaceWaterKind::Lake
            } else {
                SurfaceWaterKind::DryLand
            }
        })
        .collect()
}

fn build_lake_depth(
    original_height_cm: &[i64],
    filled_height_cm: &[i64],
    lake_owner: &[Option<usize>],
) -> Vec<f32> {
    (0..original_height_cm.len())
        .map(|index| {
            if lake_owner[index].is_some() {
                (filled_height_cm[index] - original_height_cm[index]) as f32
                    / CENTIMETERS_PER_METER as f32
            } else {
                0.0
            }
        })
        .collect()
}

fn local_runoff(
    water: &[SurfaceWaterKind],
    relative_permeability: &[f32],
    climate: &PreliminaryClimateSnapshot,
) -> Vec<[f32; CLIMATE_MONTH_COUNT]> {
    (0..water.len())
        .map(|index| {
            if water[index] == SurfaceWaterKind::Ocean {
                [0.0; CLIMATE_MONTH_COUNT]
            } else {
                let runoff_fraction = 0.85 + (0.20 - 0.85) * relative_permeability[index];
                std::array::from_fn(|month| {
                    climate.monthly_precipitation_mm().values()[index][month] * runoff_fraction
                })
            }
        })
        .collect()
}

struct WaterAccumulation {
    monthly_discharge_m3_s: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    annual_local_runoff_mm: Vec<f32>,
    mean_discharge_m3_s: Vec<f32>,
    drainage_area_km2: Vec<f32>,
}

fn accumulate_water(
    spatial: &SpatialSnapshot,
    receiver: &[Option<CellId>],
    order: &[CellId],
    monthly_local_runoff_mm: &[[f32; CLIMATE_MONTH_COUNT]],
) -> WaterAccumulation {
    let cell_count = receiver.len();
    let mut area_km2 = vec![0.0_f64; cell_count];
    let mut discharge = vec![[0.0_f64; CLIMATE_MONTH_COUNT]; cell_count];
    for index in 0..cell_count {
        let area_m2 = spatial
            .cell(CellId::from_raw(index as u32))
            .expect("validated dense spatial input contains every cell")
            .area
            .get();
        area_km2[index] = area_m2 / 1_000_000.0;
        for (stored, &runoff_mm) in discharge[index]
            .iter_mut()
            .zip(&monthly_local_runoff_mm[index])
        {
            *stored = f64::from(runoff_mm) / 1_000.0 * area_m2 / SECONDS_PER_CLIMATOLOGICAL_MONTH;
        }
    }
    for &cell in order {
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
    WaterAccumulation {
        monthly_discharge_m3_s,
        annual_local_runoff_mm,
        mean_discharge_m3_s,
        drainage_area_km2,
    }
}

fn build_lake_records(
    spatial: &SpatialSnapshot,
    lakes: &LakeRouting,
    lake_depth_m: &[f32],
) -> Result<Vec<Lake>, HydrologyGenerationError> {
    lakes
        .drafts
        .iter()
        .enumerate()
        .map(|(index, draft)| {
            let mut area_m2 = 0.0;
            let mut volume_m3 = 0.0;
            for &cell in &draft.cells {
                let area = spatial
                    .cell(cell)
                    .expect("validated lake cells exist in spatial input")
                    .area
                    .get();
                area_m2 += area;
                volume_m3 += area * f64::from(lake_depth_m[cell.raw() as usize]);
            }
            Lake::new(
                LakeId::from_raw(index as u32),
                draft.cells.clone(),
                draft.surface_elevation_m,
                area_m2 / 1_000_000.0,
                volume_m3,
                draft.outlet_cell,
                draft.downstream_cell,
            )
            .map_err(HydrologyGenerationError::Hydrology)
        })
        .collect()
}

fn build_basins(
    receiver: &[Option<CellId>],
    order: &[CellId],
    water: &[SurfaceWaterKind],
    drainage_area_km2: &[f32],
    mean_discharge_m3_s: &[f32],
) -> Result<(Vec<Option<DrainageBasinId>>, Vec<DrainageBasin>), HydrologyGenerationError> {
    let mut root = vec![None; receiver.len()];
    for &cell in order.iter().rev() {
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

    for &cell in order {
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

fn quantize_centimeters(value_m: f32) -> i64 {
    (f64::from(value_m) * CENTIMETERS_PER_METER).round() as i64
}

/// Errors returned by the pure deterministic hydrology solver.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum HydrologyGenerationError {
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
        found: f32,
    },
    /// Relative permeability is invalid.
    #[error("relative permeability {found} at {cell:?} is outside finite 0..=1")]
    PermeabilityOutOfRange {
        /// The affected cell.
        cell: CellId,
        /// The rejected permeability.
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
