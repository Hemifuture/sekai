//! B1 source-term diagnostic on the R6 terrain, not a published lake model.

use super::*;
use crate::generators::spatial::lake_probe_overlap_steradians;
use crate::world::natural::{
    formation_monthly_precipitation_mm, formation_runoff_fraction,
    lake_probe_freshwater_evaporation_kg_m2_s, CLIMATE_MONTH_COUNT,
    SECONDS_PER_CLIMATOLOGICAL_MONTH,
};
use crate::world::CellId;

struct Rates {
    precipitation_m_year: Vec<f64>,
    runoff_m_year: Vec<f64>,
    evaporation_m_year: Vec<f64>,
}

fn rates(artifact: &NaturalFormationBundleArtifact) -> Rates {
    let bundle = artifact.bundle();
    let fields = bundle.climate().fields();
    let mut precipitation =
        Vec::with_capacity(fields.monthly_precipitation_mm_day().values().len());
    let mut runoff = Vec::with_capacity(precipitation.capacity());
    let mut evaporation = Vec::with_capacity(precipitation.capacity());
    for (cell, monthly) in fields
        .monthly_precipitation_mm_day()
        .values()
        .iter()
        .enumerate()
    {
        let p = formation_monthly_precipitation_mm(monthly)
            .iter()
            .sum::<f64>()
            / 1_000.0;
        precipitation.push(p);
        runoff
            .push(p * formation_runoff_fraction(bundle.substrate().relative_permeability()[cell]));
        let annual_mm: f64 = (0..CLIMATE_MONTH_COUNT)
            .map(|month| {
                let wind = fields.near_surface_wind_m_s().values()[cell][month];
                let speed = wind
                    .into_iter()
                    .map(|v| f64::from(v).powi(2))
                    .sum::<f64>()
                    .sqrt();
                // P4's tendency wire uses kg m^-2 as mm water equivalent.
                lake_probe_freshwater_evaporation_kg_m2_s(
                    f64::from(fields.monthly_air_temperature_c().values()[cell][month]),
                    f64::from(fields.monthly_specific_humidity().values()[cell][month]),
                    speed,
                ) * SECONDS_PER_CLIMATOLOGICAL_MONTH
            })
            .sum();
        evaporation.push(annual_mm / 1_000.0);
    }
    Rates {
        precipitation_m_year: precipitation,
        runoff_m_year: runoff,
        evaporation_m_year: evaporation,
    }
}

fn old_catchment(artifact: &NaturalFormationBundleArtifact, lake: &Lake) -> Vec<bool> {
    let receivers = artifact
        .bundle()
        .surface_formation()
        .hydrology()
        .flow_receiver();
    let mut upstream = vec![Vec::new(); receivers.len()];
    for (from, to) in receivers.iter().enumerate() {
        if let Some(to) = to {
            upstream[to.raw() as usize].push(from);
        }
    }
    let mut reached = vec![false; receivers.len()];
    let mut queue: VecDeque<_> = lake
        .cells()
        .iter()
        .map(|cell| cell.raw() as usize)
        .collect();
    for &cell in &queue {
        reached[cell] = true;
    }
    while let Some(cell) = queue.pop_front() {
        for &next in &upstream[cell] {
            if !reached[next] {
                reached[next] = true;
                queue.push_back(next);
            }
        }
    }
    reached
}

struct Sample {
    wet_area_m2: f64,
    partition_area_m2: f64,
    outside_c_area_m2: f64,
    precipitation_m3_year: f64,
    evaporation_m3_year: f64,
    runoff_removed_inside_c_m3_year: f64,
    runoff_removed_outside_c_m3_year: f64,
    nonmonotone_cells: usize,
}

fn sample(
    surface: &SphericalSurfaceSnapshot,
    tin: &Tin,
    rates: &Rates,
    catchment: &[bool],
    lake: &Lake,
) -> Sample {
    let h = f64::from(lake.surface_elevation_m());
    let wet = tin.component(lake, h);
    let radius2 = surface.radius().get().powi(2);
    let mut output = Sample {
        wet_area_m2: 0.0,
        partition_area_m2: 0.0,
        outside_c_area_m2: 0.0,
        precipitation_m3_year: 0.0,
        evaporation_m3_year: 0.0,
        runoff_removed_inside_c_m3_year: 0.0,
        runoff_removed_outside_c_m3_year: 0.0,
        nonmonotone_cells: 0,
    };
    let mut counted = vec![false; tin.elevation.len()];
    for (face, indices) in tin.faces.iter().enumerate() {
        if !indices.iter().any(|&cell| wet[cell]) {
            continue;
        }
        let polygon = clip(tin.corners(face), tin.values(face).map(|z| h - z));
        let (area, _) = polygon_geometry(&polygon, radius2);
        output.wet_area_m2 += area;
        let polygon: Vec<_> = polygon.into_iter().map(|point| point.p).collect();
        let mut candidates: Vec<_> = indices.to_vec();
        for &cell in indices {
            candidates.extend(&tin.neighbours[cell]);
        }
        candidates.sort_unstable();
        candidates.dedup();
        for cell in candidates {
            let area =
                lake_probe_overlap_steradians(&polygon, surface, CellId::from_raw(cell as u32))
                    .expect("existing spherical clip accepts the local wet polygon")
                    * radius2;
            if area <= 0.0 {
                continue;
            }
            output.partition_area_m2 += area;
            output.precipitation_m3_year += area * rates.precipitation_m_year[cell];
            output.evaporation_m3_year += area * rates.evaporation_m_year[cell];
            if catchment[cell] {
                output.runoff_removed_inside_c_m3_year += area * rates.runoff_m_year[cell];
            } else {
                output.outside_c_area_m2 += area;
                output.runoff_removed_outside_c_m3_year += area * rates.runoff_m_year[cell];
            }
            if !counted[cell]
                && rates.evaporation_m_year[cell] - rates.precipitation_m_year[cell]
                    + rates.runoff_m_year[cell]
                    < 0.0
            {
                output.nonmonotone_cells += 1;
            }
            counted[cell] = true;
        }
    }
    output
}

fn report_b1(
    surface: &SphericalSurfaceSnapshot,
    artifact: &NaturalFormationBundleArtifact,
    tin: &Tin,
    rate: &Rates,
    slot: usize,
) {
    let start = Instant::now();
    let hydro = artifact.bundle().surface_formation().hydrology();
    let original = &hydro.lakes()[slot];
    let ocean: Vec<_> = (0..tin.elevation.len())
        .map(|cell| hydro.surface_water().get(cell) == Some(SurfaceWaterKind::Ocean))
        .collect();
    let (spill, _) = tin.spill(original, &ocean);
    // R6 nodes are promoted published f32 values, so this exact graph saddle
    // is representable in f32. This adapter does not apply to refined f64 H.
    assert_eq!(f64::from(spill as f32), spill);
    let lake = Lake::new(
        original.id(),
        original.cells().to_vec(),
        spill as f32,
        original.area_km2(),
        original.volume_m3(),
        original.outlet_cell(),
        original.downstream_cell(),
    )
    .expect("diagnostic lake adapter preserves local fields");
    let catchment = old_catchment(artifact, original);
    let q0: f64 = surface
        .cells()
        .iter()
        .filter(|cell| catchment[cell.id.raw() as usize])
        .map(|cell| cell.area.get() * rate.runoff_m_year[cell.id.raw() as usize])
        .sum();
    let other_lakes: Vec<_> = hydro
        .lakes()
        .iter()
        .filter(|other| {
            other.id() != original.id()
                && other
                    .cells()
                    .iter()
                    .any(|cell| catchment[cell.raw() as usize])
        })
        .map(|lake| lake.id().raw())
        .collect();
    let value = sample(surface, tin, rate, &catchment, &lake);
    let f_if_c_valid =
        q0 - value.runoff_removed_inside_c_m3_year - value.runoff_removed_outside_c_m3_year
            + value.precipitation_m3_year
            - value.evaporation_m3_year;
    let f_fixed_old_dag = q0 - value.runoff_removed_inside_c_m3_year + value.precipitation_m3_year
        - value.evaporation_m3_year;
    eprintln!("B1_DIAGNOSTIC lake={slot} H_spill={spill:.12} catchment_cells={} upstream_other_lakes={other_lakes:?} Q0_m3_year={q0:.9} wet_area_m2={:.9} partition_area_m2={:.9} partition_relative_error={:.12e} outside_C_area_m2={:.9} outside_C_fraction={:.9} P_wet_m3_year={:.9} E_wet_m3_year={:.9} R_removed_inside_C_m3_year={:.9} R_removed_outside_C_m3_year={:.9} F_legacy_M_subset_C_m3_year={f_if_c_valid:.9} F_fixed_old_DAG_m3_year={f_fixed_old_dag:.9} negative_E_minus_P_plus_R_cells={} ms={:.3}",
        catchment.iter().filter(|&&yes|yes).count(),value.wet_area_m2,value.partition_area_m2,
        (value.partition_area_m2-value.wet_area_m2)/value.wet_area_m2,value.outside_c_area_m2,value.outside_c_area_m2/value.wet_area_m2,
        value.precipitation_m3_year,value.evaporation_m3_year,value.runoff_removed_inside_c_m3_year,value.runoff_removed_outside_c_m3_year,
        value.nonmonotone_cells,start.elapsed().as_secs_f64()*1_000.0);
    eprintln!("B1_DIAGNOSTIC lake={slot} root_status=NOT_SOLVED_OLD_CATCHMENT_DIAGNOSTIC new_same_terrain_catchment_and_hierarchy_required=true old_V_is_not_inventory=true");
}

fn run_b1(profile: NaturalQualityProfile, slots: &[usize]) {
    let start = Instant::now();
    let (surface, artifact) = fixture(profile);
    eprintln!(
        "B1_DIAGNOSTIC identity=lake-b1-20261030-v1 profile={profile:?} fixture_ms={:.3}",
        start.elapsed().as_secs_f64() * 1_000.0
    );
    let tin = Tin::new(
        &surface,
        artifact
            .bundle()
            .surface_formation()
            .terrain_fields()
            .current_elevation_m(),
    );
    let start = Instant::now();
    let rates = rates(&artifact);
    eprintln!(
        "B1_DIAGNOSTIC rate_build_ms={:.3}",
        start.elapsed().as_secs_f64() * 1_000.0
    );
    for &slot in slots {
        report_b1(&surface, &artifact, &tin, &rates, slot);
    }
}

#[test]
#[ignore = "R7 three-lake source diagnostic; exclusive Release compute slot"]
fn b1_source_diagnostic_draft39_control33() {
    run_b1(NaturalQualityProfile::Draft, &[39, 33]);
}

#[test]
#[ignore = "R7 reproduced Standard55 source diagnostic; exclusive Release compute slot"]
fn b1_source_diagnostic_standard55() {
    run_b1(NaturalQualityProfile::Standard, &[55]);
}

#[test]
fn freshwater_probe_preserves_bulk_zero_flux_cases() {
    use crate::world::natural::saturation_specific_humidity_kg_kg;

    let saturation = saturation_specific_humidity_kg_kg(0.0);
    assert_eq!(
        lake_probe_freshwater_evaporation_kg_m2_s(0.0, 0.0, 0.0),
        0.0
    );
    assert_eq!(
        lake_probe_freshwater_evaporation_kg_m2_s(0.0, 2.0 * saturation, 1.0),
        0.0
    );
}
