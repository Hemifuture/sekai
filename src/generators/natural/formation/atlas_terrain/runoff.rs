//! Current rates on the fixed bed, routed on the upstream source-site graph
//! (world-truth-scope R1.2 item 5); no invented elapsed time or lake filling.
use super::{
    geometry::{site_neighbors, BuiltGeometry},
    AtlasGenerationError,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::hydrology::{
    route_monthly_discharge, upstream_to_downstream_order,
};
use crate::world::natural::atlas_terrain::AtlasSiteBody;
use crate::world::natural::{
    formation_runoff_fraction, AtlasRiverSegment, AtlasRunoffBudget, GlobalCirculationSnapshot,
    HydroErosionSpec, CLIMATE_MONTH_COUNT, MEAN_SOLAR_DAY_SECONDS,
};
use crate::world::spatial::{central_angle, SphericalSurfaceSnapshot};
use crate::world::CellId;
use civ_atlas_terrain::Terrain;
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

fn precipitation_rate_m3_s(precipitation_mm_day: f32, area_m2: f64) -> f64 {
    f64::from(precipitation_mm_day) / 1000.0 / MEAN_SOLAR_DAY_SECONDS * area_m2
}

struct CurrentRouting {
    receiver: Vec<Option<CellId>>,
    lake_root: Vec<usize>,
}

/// Steepest descent (O'Callaghan & Mark 1984) on the upstream coarse bed. Ocean
/// sites are terminals at sea level; a lake drains through its lowest non-lake
/// neighbour when that neighbour is not above the lake surface, and every lake
/// site routes to the lake site touching that outlet.
fn current_routing(
    terrain: &Terrain,
    surface: &SphericalSurfaceSnapshot,
    geometry: &BuiltGeometry,
    cancellation: &BuildCancellation,
) -> Result<CurrentRouting, AtlasGenerationError> {
    let sites = &geometry.water.sites;
    let bed = terrain.bed_elevation_m();
    let n = sites.len();
    let potential = |i: usize| match sites[i] {
        Some(AtlasSiteBody::Ocean) => 0.0,
        Some(AtlasSiteBody::Lake { surface_m, .. }) => surface_m,
        None => f64::from(bed[i]),
    };
    let direction = |i: usize| surface.cells()[i].site;
    let mut receiver = vec![None; n];
    for i in 0..n {
        if i % 256 == 0 {
            super::check_cancelled(cancellation)?;
        }
        if sites[i].is_some() {
            continue;
        }
        let mut best: Option<(usize, f64)> = None;
        for &j in site_neighbors(terrain, i) {
            let drop = potential(i) - potential(j);
            // A level step is only allowed into a terminal sea, so no cycle forms.
            if drop < 0.0 || (drop == 0.0 && sites[j] != Some(AtlasSiteBody::Ocean)) {
                continue;
            }
            let slope = drop / central_angle(direction(i), direction(j));
            if best.is_none_or(|(_, s)| slope > s) {
                best = Some((j, slope));
            }
        }
        receiver[i] = best.map(|(j, _)| CellId::from_raw(j as u32));
    }
    let mut lake_root = Vec::with_capacity(geometry.lakes.len());
    for (lake, record) in geometry.lakes.iter().enumerate() {
        let member =
            |i: usize| matches!(sites[i], Some(AtlasSiteBody::Lake { lake: l, .. }) if l == lake);
        let (root, outlet) = (0..n)
            .filter(|&i| member(i))
            .flat_map(|i| {
                site_neighbors(terrain, i)
                    .iter()
                    .filter(|&&j| !member(j))
                    .map(move |&j| (i, j))
            })
            .min_by(|a, b| potential(a.1).total_cmp(&potential(b.1)).then(a.cmp(b)))
            .ok_or_else(|| AtlasGenerationError::Geometry("lake has no shore".into()))?;
        if potential(outlet) <= record.surface_elevation_m {
            receiver[root] = Some(CellId::from_raw(outlet as u32));
        }
        let mut queue = VecDeque::from([root]);
        while let Some(i) = queue.pop_front() {
            for &j in site_neighbors(terrain, i) {
                if j != root && member(j) && receiver[j].is_none() {
                    receiver[j] = Some(CellId::from_raw(i as u32));
                    queue.push_back(j);
                }
            }
        }
        lake_root.push(root);
    }
    Ok(CurrentRouting {
        receiver,
        lake_root,
    })
}

pub(super) fn build_current_runoff(
    terrain: &Terrain,
    surface: &SphericalSurfaceSnapshot,
    geometry: &mut BuiltGeometry,
    climate: &GlobalCirculationSnapshot,
    spec: &HydroErosionSpec,
    cancellation: &BuildCancellation,
) -> Result<(Vec<AtlasRiverSegment>, AtlasRunoffBudget), AtlasGenerationError> {
    let CurrentRouting {
        receiver,
        lake_root,
    } = current_routing(terrain, surface, geometry, cancellation)?;
    let n = receiver.len();
    let order = upstream_to_downstream_order(&receiver, Some(cancellation))?;
    let precipitation = climate.fields().monthly_precipitation_mm_day().values();
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
        super::check_cancelled(cancellation)?;
        for (cell, slot) in discharge.iter_mut().enumerate() {
            let dry_area =
                surface.cells()[cell].area.get() * f64::from(geometry.fields.dry_fraction[cell]);
            let runoff = dry_runoff_rate_m3_s(
                precipitation[cell][m],
                dry_area,
                geometry.fields.relative_permeability[cell],
            );
            budget.monthly_runoff_m3_s[m] += runoff;
            *slot = [runoff];
        }
        for &(cell, lake, area) in &geometry.lake_cells {
            let rain = precipitation_rate_m3_s(precipitation[cell][m], area);
            geometry.lakes[lake].direct_monthly_precipitation_m3_s[m] += rain;
            budget.monthly_direct_lake_precipitation_m3_s[m] += rain;
            discharge[lake_root[lake]][0] += rain;
        }
        route_monthly_discharge(&receiver, &order, &mut discharge, None, Some(cancellation))?;
        for i in 0..n {
            annual_discharge_sum[i] += discharge[i][0];
            if receiver[i].is_none() {
                let terminal = match geometry.water.sites[i] {
                    Some(AtlasSiteBody::Ocean) => &mut budget.monthly_ocean_delivery_m3_s,
                    Some(AtlasSiteBody::Lake { .. }) => &mut budget.monthly_lake_retention_m3_s,
                    None => &mut budget.monthly_dry_depression_retention_m3_s,
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
        budget.monthly_routing_residual_m3_s[m] = budget.routing_residual_m3_s(m);
    }
    let is_lake = |i: usize| matches!(geometry.water.sites[i], Some(AtlasSiteBody::Lake { .. }));
    let mut rivers = Vec::new();
    for i in 0..n {
        if let Some(downstream) = receiver[i] {
            let mean = annual_discharge_sum[i] / CLIMATE_MONTH_COUNT as f64;
            if mean >= f64::from(spec.river_discharge_threshold_m3_s())
                && (!is_lake(i) || lake_root.contains(&i))
            {
                rivers.push(AtlasRiverSegment {
                    endpoints: [
                        surface.cells()[i].site.components(),
                        surface.cells()[downstream.raw() as usize].site.components(),
                    ],
                    mean_discharge_m3_s: mean,
                });
            }
        }
    }
    Ok((rivers, budget))
}
