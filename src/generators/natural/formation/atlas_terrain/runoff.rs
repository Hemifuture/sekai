//! Current rates on the initialized bed; no invented elapsed time or lake filling.
use super::{geometry::BuiltGeometry, AtlasGenerationError};
use crate::engine::BuildCancellation;
use crate::generators::natural::hydrology::{
    route_monthly_discharge, upstream_to_downstream_order,
};
use crate::world::natural::{
    formation_runoff_fraction, AtlasRiverSegment, AtlasRunoffBudget, GlobalCirculationSnapshot,
    HydroErosionSpec, CLIMATE_MONTH_COUNT, MEAN_SOLAR_DAY_SECONDS,
};
use crate::world::CellId;
use std::collections::VecDeque;

pub(super) fn dry_runoff_rate_m3_s(
    precipitation_mm_day: f32,
    dry_area_m2: f64,
    permeability: f32,
) -> f64 {
    f64::from(precipitation_mm_day) / 1000.0 / MEAN_SOLAR_DAY_SECONDS
        * dry_area_m2
        * formation_runoff_fraction(permeability)
}

pub(super) struct CurrentRouting {
    pub receiver: Vec<Option<CellId>>,
    pub lake_root: Vec<usize>,
}

pub(super) fn current_routing(
    geometry: &BuiltGeometry,
    cancellation: &BuildCancellation,
) -> Result<CurrentRouting, AtlasGenerationError> {
    let support = &geometry.support;
    let water = &geometry.water;
    let n = support.bed.len();
    let mut lake_root: Vec<Option<usize>> = vec![None; geometry.lakes.len()];
    for (i, &lake) in water.lake.iter().enumerate() {
        if let Some(lake) = lake {
            if lake_root[lake].is_none_or(|root| water.rank[i] < water.rank[root]) {
                lake_root[lake] = Some(i);
            }
        }
    }
    let lake_root = lake_root
        .into_iter()
        .map(|root| root.expect("each initialized lake owns support nodes"))
        .collect::<Vec<_>>();
    let potential = |i: usize| water.level[i].unwrap_or(support.bed[i]);
    let rank = |i: usize| water.lake[i].map_or(water.rank[i], |lake| water.rank[lake_root[lake]]);
    let mut receiver = vec![None; n];
    for (i, assigned_receiver) in receiver.iter_mut().enumerate() {
        if i % 256 == 0 {
            super::check_cancelled(cancellation)?;
        }
        if water.ocean[i] || water.lake[i].is_some() {
            continue;
        }
        // Steepest descent on the actual current exposed surface. A dry unseeded
        // depression stores its incoming rate; Priority-Flood is not permission
        // to invent the water needed to raise it to its spill.
        let mut best = None;
        let mut best_slope = 0.0;
        for &j in geometry.neighbors.neighbors(i) {
            let j = j as usize;
            let drop = potential(i) - potential(j);
            if drop < 0.0 || (drop == 0.0 && rank(j) >= rank(i)) {
                continue;
            }
            let distance =
                crate::world::spatial::central_angle(support.directions[i], support.directions[j]);
            let slope = drop / distance;
            if best.is_none() || slope > best_slope {
                best = Some(CellId::from_raw(j as u32));
                best_slope = slope;
            }
        }
        *assigned_receiver = best;
    }
    for (lake, &root) in lake_root.iter().enumerate() {
        let record = &geometry.lakes[lake];
        if record.surface_elevation_m == record.spill_elevation_m {
            receiver[root] = geometry
                .neighbors
                .neighbors(root)
                .iter()
                .map(|&j| j as usize)
                .filter(|&j| {
                    water.lake[j] != Some(lake)
                        && water.rank[j] < water.rank[root]
                        && support.bed[j] <= record.surface_elevation_m
                })
                .min_by_key(|&j| water.rank[j])
                .map(|j| CellId::from_raw(j as u32));
            if receiver[root].is_none() {
                return Err(AtlasGenerationError::Geometry(
                    "full lake has no actual spill edge".into(),
                ));
            }
        }
        let mut queue = VecDeque::from([root]);
        while let Some(i) = queue.pop_front() {
            for &j in geometry.neighbors.neighbors(i) {
                let j = j as usize;
                if j != root && water.lake[j] == Some(lake) && receiver[j].is_none() {
                    receiver[j] = Some(CellId::from_raw(i as u32));
                    queue.push_back(j);
                }
            }
        }
    }
    Ok(CurrentRouting {
        receiver,
        lake_root,
    })
}

pub(super) fn build_current_runoff(
    geometry: &mut BuiltGeometry,
    climate: &GlobalCirculationSnapshot,
    spec: &HydroErosionSpec,
    cancellation: &BuildCancellation,
) -> Result<(Vec<AtlasRiverSegment>, AtlasRunoffBudget), AtlasGenerationError> {
    let CurrentRouting {
        receiver,
        lake_root,
    } = current_routing(geometry, cancellation)?;
    let support = &geometry.support;
    let water = &geometry.water;
    let n = support.bed.len();
    let order = upstream_to_downstream_order(&receiver, Some(cancellation))?;
    let precipitation = climate.fields().monthly_precipitation_mm_day().values();
    // Only channel means and lake monthly rates are published. Streaming one
    // month avoids retaining twelve large support-node arrays with no consumer.
    let mut discharge = vec![[0.0; 1]; n];
    let mut annual_discharge_sum = vec![0.0; n];
    let mut budget = AtlasRunoffBudget {
        monthly_runoff_m3_s: [0.0; CLIMATE_MONTH_COUNT],
        monthly_direct_lake_precipitation_m3_s: [0.0; CLIMATE_MONTH_COUNT],
        monthly_ocean_delivery_m3_s: [0.0; CLIMATE_MONTH_COUNT],
        monthly_lake_retention_m3_s: [0.0; CLIMATE_MONTH_COUNT],
        monthly_dry_depression_retention_m3_s: [0.0; CLIMATE_MONTH_COUNT],
        monthly_routing_residual_m3_s: [0.0; CLIMATE_MONTH_COUNT],
    };
    for m in 0..CLIMATE_MONTH_COUNT {
        discharge.fill([0.0]);
        for (index, face) in support.faces.iter().enumerate() {
            if index % 256 == 0 {
                super::check_cancelled(cancellation)?;
            }
            let dry_area = face.area_m2 * (1.0 - face.ocean_fraction - face.lake_fraction);
            let runoff = dry_runoff_rate_m3_s(
                precipitation[face.owner_index()][m],
                dry_area,
                geometry.fields.relative_permeability[face.owner_index()],
            );
            budget.monthly_runoff_m3_s[m] += runoff;
            for &node in &face.nodes {
                discharge[node as usize][0] += runoff / 3.0;
            }
            if let Some(lake) = face.lake {
                let rain = f64::from(precipitation[face.owner_index()][m])
                    / 1000.0
                    / MEAN_SOLAR_DAY_SECONDS
                    * face.area_m2
                    * face.lake_fraction;
                geometry.lakes[lake].direct_monthly_precipitation_m3_s[m] += rain;
                budget.monthly_direct_lake_precipitation_m3_s[m] += rain;
                discharge[lake_root[lake]][0] += rain;
            }
        }
        route_monthly_discharge(&receiver, &order, &mut discharge, None, Some(cancellation))?;
        for i in 0..n {
            if i % 256 == 0 {
                super::check_cancelled(cancellation)?;
            }
            annual_discharge_sum[i] += discharge[i][0];
            if receiver[i].is_none() {
                let terminal = if water.ocean[i] {
                    &mut budget.monthly_ocean_delivery_m3_s
                } else if water.lake[i].is_some() {
                    &mut budget.monthly_lake_retention_m3_s
                } else {
                    &mut budget.monthly_dry_depression_retention_m3_s
                };
                terminal[m] += discharge[i][0];
            }
        }
        for (lake, &root) in lake_root.iter().enumerate() {
            let record = &mut geometry.lakes[lake];
            record.resolved_monthly_inflow_m3_s[m] =
                discharge[root][0] - record.direct_monthly_precipitation_m3_s[m];
            if receiver[root].is_some() {
                record.resolved_monthly_outflow_m3_s[m] = discharge[root][0];
            }
        }
        budget.monthly_routing_residual_m3_s[m] = budget.monthly_runoff_m3_s[m]
            + budget.monthly_direct_lake_precipitation_m3_s[m]
            - budget.monthly_ocean_delivery_m3_s[m]
            - budget.monthly_lake_retention_m3_s[m]
            - budget.monthly_dry_depression_retention_m3_s[m];
    }
    let mut rivers = Vec::new();
    for i in 0..n {
        if i % 256 == 0 {
            super::check_cancelled(cancellation)?;
        }
        if let Some(downstream) = receiver[i] {
            let mean = annual_discharge_sum[i] / CLIMATE_MONTH_COUNT as f64;
            if mean >= f64::from(spec.river_discharge_threshold_m3_s())
                && water.lake[i].is_none_or(|lake| lake_root[lake] == i)
            {
                rivers.push(AtlasRiverSegment {
                    endpoints: [
                        support.directions[i].components(),
                        support.directions[downstream.raw() as usize].components(),
                    ],
                    mean_discharge_m3_s: mean,
                });
            }
        }
    }
    Ok((rivers, budget))
}
