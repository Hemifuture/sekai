//! Ignored Release probe for the A10 reduced-order climate prototype.
//!
//! `SEKAI_A10_PROFILE=draft|standard` selects the profile (seed 42) and
//! `SEKAI_A10_DIR` the output directory for the JSON evidence. The probe
//! builds the production chain once (baseline P4 start, P5, endpoint P4),
//! then evaluates the reduced model on (a) the baseline endpoint forcing, for
//! a climate-only comparison and timing, and (b) its own start -> P5 ->
//! endpoint chain, for the downstream P5 effect.
//!
//! The probe wraps reduced fields in a C2-shaped `GlobalCirculationSnapshot`
//! only so production consumers (P5 kernels, hydrology, the P4 quality
//! evaluator) can read them unchanged. The wrapper carries the production
//! model fingerprint and placeholder vertical fields; it never leaves the
//! probe and is not a proposed schema.

mod latent;
mod moisture;

use std::time::Instant;

use serde_json::{json, Value};

use super::{
    dot, norm, solve_reduced_climate, vapour_column_mass_kg_m2, IceAlbedoFeedback,
    OutgoingLongwaveClosure, ReducedClimateGeometry, ReducedInputs, ReducedPhysics,
    ReducedWorkClimate,
};
use crate::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use crate::generators::natural::formation::global_circulation::{
    global_circulation_model_fingerprint, project_intensive_scalar_cancellable,
    project_monthly_extensive_rate_cancellable, project_monthly_intensive_scalar_cancellable,
    project_monthly_tangent_vectors_cancellable, GlobalClimateForcing,
    SELECTED_PRODUCTION_INTEGRATOR,
};
use crate::generators::natural::formation::reduced_climate::forcing::{
    ReducedClimateForcing, ReducedClimateTerrain,
};
use crate::generators::natural::formation::surface_formation::generation::{
    advance_surface_processes, SurfaceProcessInputs,
};
use crate::generators::natural::formation::surface_formation::FormationState;
use crate::generators::natural::random::LabeledSubstreams;
use crate::generators::natural::surface_water_geometry::SurfaceWaterStencil;
use crate::generators::natural::{
    quality, ClimateWorkDomainArtifact, EvolvedTectonicGenerator, FormationHydrologyGenerator,
    GeologicSubstrateGenerator, GlobalCirculationGenerator, GlobalClimateForcingBuilder,
    PrimaryReliefGenerator, SurfaceFormationGenerator, SurfaceFormationInputs,
};
use crate::generators::spatial::ProfileSurfaceBuilder;
use crate::world::natural::{
    expected_global_circulation_dense_state_bytes, formation_monthly_land_runoff_mm,
    formation_monthly_precipitation_mm, formation_runoff_fraction,
    saturation_specific_humidity_kg_kg, ClimateBudgetReport, ClimateCapabilitySet,
    ClimateCheckpoint, ClimateLayerLayout, ClimateModelProfile, ClimateQuantizationId,
    ClimateRemapReport, ClimateSolveReport, ClimateSpec, ClimateWorkDomainSnapshot, GeologicSpec,
    GlobalCirculationFields, GlobalCirculationSnapshot, HydroErosionSpec, MonthlyScalarField,
    MonthlyVector3Field, NaturalQualityProfile, NaturalQualityReport, ReliefSpec,
    ResolvedWorldFormation, ResolvedWorldFormationPreset, SphericalHydrologySnapshot,
    SurfaceWaterGeometry, TectonicSpec, WorldFormationPreset, CLIMATE_MONTH_COUNT,
    CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M, EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2,
    GLOBAL_CIRCULATION_MINIMUM_FAST_SUBSTEPS_PER_MACRO_STEP, GLOBAL_CIRCULATION_SCHEMA_V2,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1, SURFACE_FORMATION_HORIZON_YEARS,
};
use crate::world::spatial::{canonical_east_north_basis, SphericalSurfaceSnapshot, SurfaceRef};
use crate::world::{Meters, RootSeed};

const MONTHS: usize = CLIMATE_MONTH_COUNT;
const TIMING_REPETITIONS: usize = 5;
/// Latitude band width of the zonal-profile evidence.
const ZONAL_BAND_DEGREES: f64 = 5.0;

type Monthly = Vec<[f32; MONTHS]>;
type MonthlyVector = Vec<[[f32; 3]; MONTHS]>;

/// Published (authoritative-surface) climate as consumed downstream.
struct Published {
    wind: MonthlyVector,
    current: MonthlyVector,
    air: Monthly,
    sst: Monthly,
    albedo: Vec<f32>,
    asr: Monthly,
    olr: Monthly,
    humidity: Monthly,
    evaporation: Monthly,
    precipitation: Monthly,
    orographic: Monthly,
    sea_height: Monthly,
}

/// Authoritative reference heights `h_c` and the scheme-C work mean `H = M h`.
/// The production minimal forcing (design section 10 T3) of one terrain.
fn reduced_forcing(
    domain: &ClimateWorkDomainArtifact,
    elevation_m: &[f32],
    water: &SurfaceWaterGeometry,
    relative_permeability: &[f32],
    spec: &ClimateSpec,
) -> ReducedClimateForcing {
    ReducedClimateForcing::build(
        domain.snapshot(),
        domain.reduced_climate_geometry(),
        ReducedClimateTerrain {
            elevation_m,
            surface_water_geometry: water,
            relative_permeability,
        },
        spec,
        &BuildCancellation::new(),
    )
    .expect("reduced forcing")
}

fn to_monthly(values: &[[f64; MONTHS]]) -> Monthly {
    values.iter().map(|m| m.map(|v| v as f32)).collect()
}

fn to_monthly_vector(values: &[[[f64; 3]; MONTHS]]) -> MonthlyVector {
    values
        .iter()
        .map(|m| m.map(|v| v.map(|c| c as f32)))
        .collect()
}

/// Projects every published field, applying the LOD-2 scheme-C lapse-rate
/// projection to air temperature and RH-preserving humidity (LOD-2 §3.1, §11.4).
fn project_all(
    surface: &SphericalSurfaceSnapshot,
    domain: &ClimateWorkDomainSnapshot,
    forcing: &ReducedClimateForcing,
    work: &ReducedWorkClimate,
    cell_heights: &[f64],
    cancellation: &BuildCancellation,
) -> Published {
    let mut published = project_for_p5(surface, domain, forcing, work, cancellation);
    let theta = project_monthly_intensive_scalar_cancellable(
        domain,
        &to_monthly(&work.theta_c),
        cancellation,
    )
    .expect("theta projection")
    .into_values();
    let air: Monthly = theta
        .iter()
        .zip(cell_heights)
        .map(|(months, h)| {
            months.map(|t| (f64::from(t) - CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M * h) as f32)
        })
        .collect();
    let relative_humidity: Vec<[f64; MONTHS]> = work
        .specific_humidity
        .iter()
        .zip(&work.air_temperature_c)
        .map(|(q, t)| std::array::from_fn(|m| q[m] / saturation_specific_humidity_kg_kg(t[m])))
        .collect();
    let rh = project_monthly_intensive_scalar_cancellable(
        domain,
        &to_monthly(&relative_humidity),
        cancellation,
    )
    .expect("rh projection")
    .into_values();
    let humidity: Monthly = rh
        .iter()
        .zip(&air)
        .map(|(r, t)| {
            std::array::from_fn(|m| {
                (f64::from(r[m]) * saturation_specific_humidity_kg_kg(f64::from(t[m]))) as f32
            })
        })
        .collect();
    let extensive = |values: &[[f64; MONTHS]]| {
        project_monthly_extensive_rate_cancellable(domain, &to_monthly(values), cancellation)
            .expect("extensive projection")
            .into_values()
    };
    let intensive = |values: &[[f64; MONTHS]]| {
        project_monthly_intensive_scalar_cancellable(domain, &to_monthly(values), cancellation)
            .expect("intensive projection")
            .into_values()
    };
    let asr: Vec<[f64; MONTHS]> = work.monthly_absorbed_shortwave_w_m2.clone();
    let albedo_work: Vec<f32> = work.surface_albedo.iter().map(|&v| v as f32).collect();
    published.air = air;
    published.sst = intensive(&work.sea_surface_temperature_c);
    published.albedo =
        project_intensive_scalar_cancellable(domain, &albedo_work, cancellation).expect("albedo");
    published.asr = extensive(&asr);
    published.olr = extensive(&work.outgoing_longwave_w_m2);
    published.humidity = humidity;
    published.evaporation = extensive(&work.evaporation_mm_day);
    published.orographic = extensive(&work.orographic_precipitation_mm_day);
    published.sea_height = intensive(&work.sea_surface_height_m);
    published
}

/// Projects only what P5 reads (precipitation, wind, ocean current).
fn project_for_p5(
    surface: &SphericalSurfaceSnapshot,
    domain: &ClimateWorkDomainSnapshot,
    forcing: &ReducedClimateForcing,
    work: &ReducedWorkClimate,
    cancellation: &BuildCancellation,
) -> Published {
    let precipitation = project_monthly_extensive_rate_cancellable(
        domain,
        &to_monthly(&work.precipitation_mm_day),
        cancellation,
    )
    .expect("precipitation projection")
    .into_values();
    let wind = project_monthly_tangent_vectors_cancellable(
        domain,
        surface,
        &to_monthly_vector(&work.wind_m_s),
        cancellation,
    )
    .expect("wind projection");
    let mut current = project_monthly_tangent_vectors_cancellable(
        domain,
        surface,
        &to_monthly_vector(&work.ocean_current_m_s),
        cancellation,
    )
    .expect("current projection");
    for (vectors, &land) in current.iter_mut().zip(&forcing.cell_publishes_land) {
        if land {
            *vectors = [[0.0; 3]; MONTHS];
        }
    }
    let n = surface.cells().len();
    Published {
        wind,
        current,
        air: vec![[0.0; MONTHS]; n],
        sst: vec![[0.0; MONTHS]; n],
        albedo: vec![0.0; n],
        asr: vec![[0.0; MONTHS]; n],
        olr: vec![[0.0; MONTHS]; n],
        humidity: vec![[0.0; MONTHS]; n],
        evaporation: vec![[0.0; MONTHS]; n],
        precipitation,
        orographic: vec![[0.0; MONTHS]; n],
        sea_height: vec![[0.0; MONTHS]; n],
    }
}

fn published_from_snapshot(snapshot: &GlobalCirculationSnapshot) -> Published {
    let f = snapshot.fields();
    Published {
        wind: f.near_surface_wind_m_s().values().to_vec(),
        current: f.surface_ocean_current_m_s().values().to_vec(),
        air: f.monthly_air_temperature_c().values().to_vec(),
        sst: f.monthly_sea_surface_temperature_c().values().to_vec(),
        albedo: f.surface_albedo().to_vec(),
        asr: f.monthly_absorbed_shortwave_w_m2().values().to_vec(),
        olr: f.monthly_outgoing_longwave_w_m2().values().to_vec(),
        humidity: f.monthly_specific_humidity().values().to_vec(),
        evaporation: f.monthly_evaporation_mm_day().values().to_vec(),
        precipitation: f.monthly_precipitation_mm_day().values().to_vec(),
        orographic: f
            .monthly_orographic_precipitation_mm_day()
            .values()
            .to_vec(),
        sea_height: f.monthly_sea_surface_height_anomaly_m().values().to_vec(),
    }
}

/// Global budget numbers on the work grid (production `final_cycle_budget`
/// semantics: area-month means, CERES planetary albedo of the surface albedo).
fn budget_report(
    grid: &ReducedClimateGeometry,
    forcing: &ReducedClimateForcing,
    work: &ReducedWorkClimate,
) -> ClimateBudgetReport {
    let (mut asr, mut olr, mut incoming, mut reflected, mut weight) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for cell in 0..grid.cell_count() {
        let area = grid.area_m2[cell];
        let reflectance = work.diagnostics.atmospheric_shortwave_reflectance;
        let planetary =
            reflectance + (1.0 - reflectance) * work.surface_albedo[cell].clamp(0.0, 1.0);
        for month in 0..MONTHS {
            asr += area * work.monthly_absorbed_shortwave_w_m2[cell][month];
            olr += area * work.outgoing_longwave_w_m2[cell][month];
            let top = EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2
                * forcing.monthly_insolation_fraction[cell][month];
            incoming += area * top;
            reflected += area * top * planetary;
            weight += area;
        }
    }
    let d = &work.diagnostics;
    ClimateBudgetReport::new_with_climatology(
        0.0,
        0.0,
        d.water_relative_imbalance,
        d.toa_relative_error,
        0.0,
        d.evaporation_global_mean_mm_day,
        d.precipitation_global_mean_mm_day,
        asr / weight,
        olr / weight,
        reflected / incoming,
    )
    .expect("reduced budget passes the production report validation")
}

/// Wraps published fields into the C2-shaped snapshot read by P5 and quality.
/// Vertical fields are placeholders: the upper wind is the near-surface
/// steering wind (the proposed replacement for the orographic gates), shear
/// and heights are zero, thermocline fields copy the SST.
fn wrap(
    surface: &SphericalSurfaceSnapshot,
    domain: &ClimateWorkDomainSnapshot,
    forcing: &GlobalClimateForcing,
    published: &Published,
    budget: ClimateBudgetReport,
) -> GlobalCirculationSnapshot {
    let n = surface.cells().len();
    let zeros: Monthly = vec![[0.0; MONTHS]; n];
    let profile = ClimateModelProfile::C2LayeredV1;
    let thermocline_depth: Monthly = vec![[900.0; MONTHS]; n];
    let scalar = |v: &Monthly| MonthlyScalarField::from_values(v.clone()).expect("finite scalar");
    let vector =
        |v: &MonthlyVector| MonthlyVector3Field::from_values(v.clone()).expect("finite vector");
    let fields = GlobalCirculationFields::new_c2(
        vector(&published.wind),
        vector(&published.wind),
        vector(&vec![[[0.0; 3]; MONTHS]; n]),
        vector(&published.current),
        scalar(&published.air),
        scalar(&published.sst),
        published.albedo.clone(),
        scalar(&published.asr),
        scalar(&published.olr),
        scalar(&published.sst),
        scalar(&thermocline_depth),
        scalar(&published.humidity),
        scalar(&published.evaporation),
        scalar(&published.precipitation),
        scalar(&published.orographic),
        scalar(&zeros),
        scalar(&zeros),
        scalar(&published.sea_height),
        scalar(&zeros),
        scalar(&published.sst),
    )
    .expect("C2-shaped probe fields");
    let checkpoint = ClimateCheckpoint::new(
        domain.profile(),
        profile,
        SELECTED_PRODUCTION_INTEGRATOR,
        *domain.climate_grid_fingerprint(),
        *forcing.fingerprint(),
        global_circulation_model_fingerprint(profile),
        *blake3::hash(b"sekai.a10-reduced-climate-probe.v1").as_bytes(),
        ClimateQuantizationId::DeterministicF64V1,
        MONTHS as u32,
        fields.fingerprint(),
    )
    .expect("probe checkpoint");
    let solve = ClimateSolveReport::new(
        1,
        MONTHS as u64,
        MONTHS as u64 * GLOBAL_CIRCULATION_MINIMUM_FAST_SUBSTEPS_PER_MACRO_STEP,
        0,
        0.0,
        0.0,
        0.0,
        expected_global_circulation_dense_state_bytes(domain.profile(), profile, n as u32)
            .expect("dense bytes"),
    )
    .expect("probe solve report");
    let forward = domain.source_to_climate();
    let reverse = domain.climate_to_source();
    let remap = ClimateRemapReport::new(
        forward.solve_stats().max_source_margin_relative_error(),
        forward.solve_stats().max_target_margin_relative_error(),
        reverse.solve_stats().max_source_margin_relative_error(),
        reverse.solve_stats().max_target_margin_relative_error(),
        0.0,
        forward.overlap_count() as u32,
        reverse.overlap_count() as u32,
    )
    .expect("probe remap report");
    let snapshot = GlobalCirculationSnapshot::new(
        GLOBAL_CIRCULATION_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        ClimateLayerLayout::for_profile(profile),
        SELECTED_PRODUCTION_INTEGRATOR,
        ClimateCapabilitySet::for_profile(profile),
        checkpoint,
        solve,
        budget,
        remap,
        fields,
    )
    .expect("probe snapshot");
    snapshot
        .validate_against(surface)
        .expect("probe snapshot binds the authoritative surface");
    snapshot
}

fn metrics_json(report: &NaturalQualityReport) -> Value {
    Value::Object(
        report
            .metrics()
            .iter()
            .map(|m| {
                (
                    m.id().name().to_owned(),
                    json!({
                        "status": format!("{:?}", m.status()),
                        "value": m.value(),
                        "min": m.bounds().min(),
                        "max": m.bounds().max(),
                        "samples": m.sample_count(),
                    }),
                )
            })
            .collect(),
    )
}

fn annual(values: &[f32; MONTHS]) -> f64 {
    values.iter().map(|&v| f64::from(v)).sum::<f64>() / MONTHS as f64
}

fn annual_vector(values: &[[f32; 3]; MONTHS]) -> [f64; 3] {
    let mut sum = [0.0; 3];
    for v in values {
        for (s, c) in sum.iter_mut().zip(v) {
            *s += f64::from(*c) / MONTHS as f64;
        }
    }
    sum
}

/// Area-weighted RMS, bias, and pattern correlation of two cell fields.
fn compare_scalar(areas: &[f64], mask: &[bool], a: &[f64], b: &[f64]) -> Value {
    let (mut w, mut sa, mut sb) = (0.0, 0.0, 0.0);
    for i in 0..a.len() {
        if mask[i] {
            w += areas[i];
            sa += areas[i] * a[i];
            sb += areas[i] * b[i];
        }
    }
    let (ma, mb) = (sa / w, sb / w);
    let (mut sq, mut cov, mut va, mut vb) = (0.0, 0.0, 0.0, 0.0);
    for i in 0..a.len() {
        if mask[i] {
            let (da, db) = (a[i] - ma, b[i] - mb);
            sq += areas[i] * (a[i] - b[i]).powi(2);
            cov += areas[i] * da * db;
            va += areas[i] * da * da;
            vb += areas[i] * db * db;
        }
    }
    json!({
        "reduced_mean": ma,
        "baseline_mean": mb,
        "bias": ma - mb,
        "rms_difference": (sq / w).sqrt(),
        "baseline_spatial_std": (vb / w).sqrt(),
        "reduced_spatial_std": (va / w).sqrt(),
        "pattern_correlation": cov / (va.sqrt() * vb.sqrt()).max(f64::MIN_POSITIVE),
    })
}

fn zonal_profile(surface: &SphericalSurfaceSnapshot, values: &[f64]) -> Vec<f64> {
    let mut sums = [0.0_f64; 18];
    let mut weights = [0.0_f64; 18];
    for (cell, v) in surface.cells().iter().zip(values) {
        let lat = cell.centroid.components()[2].asin().to_degrees();
        let band = (((lat + 90.0) / 10.0).floor() as usize).min(17);
        sums[band] += cell.area.get() * v;
        weights[band] += cell.area.get();
    }
    sums.iter()
        .zip(weights)
        .map(|(s, w)| s / w.max(f64::MIN_POSITIVE))
        .collect()
}

/// Climate-only comparison on the authoritative surface.
fn compare_published(
    surface: &SphericalSurfaceSnapshot,
    land: &[bool],
    reduced: &Published,
    baseline: &Published,
) -> Value {
    let areas: Vec<f64> = surface.cells().iter().map(|c| c.area.get()).collect();
    let all = vec![true; areas.len()];
    let field = |p: &Published, pick: &dyn Fn(&Published, usize) -> f64| -> Vec<f64> {
        (0..areas.len()).map(|i| pick(p, i)).collect()
    };
    let air = |p: &Published, i: usize| annual(&p.air[i]);
    let precipitation = |p: &Published, i: usize| annual(&p.precipitation[i]);
    let east_wind = |p: &Published, i: usize| {
        let (east, _) = canonical_east_north_basis(surface.cells()[i].centroid);
        dot(annual_vector(&p.wind[i]), east)
    };
    let north_wind = |p: &Published, i: usize| {
        let (_, north) = canonical_east_north_basis(surface.cells()[i].centroid);
        dot(annual_vector(&p.wind[i]), north)
    };
    let jul_minus_jan = |p: &Published, i: usize| f64::from(p.air[i][6] - p.air[i][0]);
    let sst_jul_minus_jan = |p: &Published, i: usize| f64::from(p.sst[i][6] - p.sst[i][0]);
    let current_speed = |p: &Published, i: usize| norm(annual_vector(&p.current[i]));
    let mut monthly_t = 0.0;
    let mut monthly_p = 0.0;
    let mut wind_vector = 0.0;
    let mut total = 0.0;
    for (i, area) in areas.iter().enumerate() {
        for m in 0..MONTHS {
            monthly_t += area * f64::from(reduced.air[i][m] - baseline.air[i][m]).powi(2);
            monthly_p += area
                * f64::from(reduced.precipitation[i][m] - baseline.precipitation[i][m]).powi(2);
            let d: Vec<f64> = (0..3)
                .map(|c| f64::from(reduced.wind[i][m][c] - baseline.wind[i][m][c]))
                .collect();
            wind_vector += area * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
        }
        total += area * MONTHS as f64;
    }
    let ocean: Vec<bool> = land.iter().map(|l| !l).collect();
    json!({
        "annual_air_temperature_c": compare_scalar(&areas, &all, &field(reduced, &air), &field(baseline, &air)),
        "annual_air_temperature_c_land": compare_scalar(&areas, land, &field(reduced, &air), &field(baseline, &air)),
        "annual_precipitation_mm_day": compare_scalar(&areas, &all, &field(reduced, &precipitation), &field(baseline, &precipitation)),
        "annual_precipitation_mm_day_land": compare_scalar(&areas, land, &field(reduced, &precipitation), &field(baseline, &precipitation)),
        "annual_east_wind_m_s": compare_scalar(&areas, &all, &field(reduced, &east_wind), &field(baseline, &east_wind)),
        "annual_north_wind_m_s": compare_scalar(&areas, &all, &field(reduced, &north_wind), &field(baseline, &north_wind)),
        "air_jul_minus_jan_c": compare_scalar(&areas, &all, &field(reduced, &jul_minus_jan), &field(baseline, &jul_minus_jan)),
        "sst_jul_minus_jan_c_ocean": compare_scalar(&areas, &ocean, &field(reduced, &sst_jul_minus_jan), &field(baseline, &sst_jul_minus_jan)),
        "annual_current_speed_m_s_ocean": compare_scalar(&areas, &ocean, &field(reduced, &current_speed), &field(baseline, &current_speed)),
        "monthly_air_temperature_rms_c": (monthly_t / total).sqrt(),
        "monthly_precipitation_rms_mm_day": (monthly_p / total).sqrt(),
        "monthly_wind_vector_rms_m_s": (wind_vector / total).sqrt(),
        "zonal_10deg_south_to_north": {
            "air_reduced": zonal_profile(surface, &field(reduced, &air)),
            "air_baseline": zonal_profile(surface, &field(baseline, &air)),
            "precipitation_reduced": zonal_profile(surface, &field(reduced, &precipitation)),
            "precipitation_baseline": zonal_profile(surface, &field(baseline, &precipitation)),
            "east_wind_reduced": zonal_profile(surface, &field(reduced, &east_wind)),
            "east_wind_baseline": zonal_profile(surface, &field(baseline, &east_wind)),
            "north_wind_reduced": zonal_profile(surface, &field(reduced, &north_wind)),
            "north_wind_baseline": zonal_profile(surface, &field(baseline, &north_wind)),
            "air_jul_minus_jan_reduced": zonal_profile(surface, &field(reduced, &jul_minus_jan)),
            "air_jul_minus_jan_baseline": zonal_profile(surface, &field(baseline, &jul_minus_jan)),
        },
    })
}

/// River-network and runoff differences of two hydrology products.
fn compare_hydrology(
    surface: &SphericalSurfaceSnapshot,
    reduced: &SphericalHydrologySnapshot,
    baseline: &SphericalHydrologySnapshot,
) -> Value {
    let areas: Vec<f64> = surface.cells().iter().map(|c| c.area.get()).collect();
    let (mut both, mut same) = (0usize, 0usize);
    for (a, b) in reduced.flow_receiver().iter().zip(baseline.flow_receiver()) {
        if let (Some(a), Some(b)) = (a, b) {
            both += 1;
            same += usize::from(a == b);
        }
    }
    let runoff_km3 = |h: &SphericalHydrologySnapshot| {
        h.annual_local_runoff_mm()
            .iter()
            .zip(&areas)
            .map(|(r, a)| f64::from(*r) / 1.0e3 * a / 1.0e9)
            .sum::<f64>()
    };
    let qb = baseline.mean_annual_discharge_m3_s();
    let qr = reduced.mean_annual_discharge_m3_s();
    let mut sorted: Vec<f32> = qb.iter().copied().filter(|q| *q > 0.0).collect();
    sorted.sort_by(f32::total_cmp);
    let threshold = sorted.get(sorted.len() * 9 / 10).copied().unwrap_or(0.0);
    let mut ratios: Vec<f64> = qb
        .iter()
        .zip(qr)
        .filter(|(b, r)| **b >= threshold && **b > 0.0 && **r > 0.0)
        .map(|(b, r)| (f64::from(*r) / f64::from(*b)).log10())
        .collect();
    ratios.sort_by(f64::total_cmp);
    let pick = |q: f64| {
        ratios
            .get(((ratios.len() as f64 - 1.0) * q) as usize)
            .copied()
    };
    let lakes = |h: &SphericalHydrologySnapshot| h.lakes().len();
    json!({
        "routed_cells_both": both,
        "identical_flow_receiver_fraction": same as f64 / both.max(1) as f64,
        "river_segments_reduced": reduced.river_segments().len(),
        "river_segments_baseline": baseline.river_segments().len(),
        "lakes_reduced": lakes(reduced),
        "lakes_baseline": lakes(baseline),
        "annual_runoff_km3_reduced": runoff_km3(reduced),
        "annual_runoff_km3_baseline": runoff_km3(baseline),
        "top_decile_discharge_threshold_m3_s": threshold,
        "top_decile_discharge_log10_ratio_p10_p50_p90": [pick(0.1), pick(0.5), pick(0.9)],
        "top_decile_discharge_cells": ratios.len(),
    })
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

#[test]
#[ignore = "release-only A10 reduced-climate probe"]
fn a10_reduced_climate_probe() {
    run_a10_probe(false);
}

#[test]
#[ignore = "release-only fixed-flow latent-energy diagnostic"]
fn a10_fixed_flow_latent_flux_probe() {
    run_a10_probe(true);
}

#[test]
#[ignore = "release-only monthly latent-storage scale diagnostic"]
fn a10_monthly_latent_storage_probe() {
    run_a10_probe(true);
}

fn run_a10_probe(latent_only: bool) {
    let profile = match std::env::var("SEKAI_A10_PROFILE").as_deref() {
        Ok("standard") => NaturalQualityProfile::Standard,
        _ => NaturalQualityProfile::Draft,
    };
    let out_dir = std::path::PathBuf::from(
        std::env::var("SEKAI_A10_DIR")
            .unwrap_or_else(|_| "D:/hemifuture-probes/a10-climate-reduced".into()),
    );
    std::fs::create_dir_all(&out_dir).expect("output directory");
    let longwave = match std::env::var("SEKAI_A10_OLR").as_deref() {
        Ok("budyko") => OutgoingLongwaveClosure::BudykoTransEbm,
        _ => OutgoingLongwaveClosure::ProductionGray,
    };
    let ice = match std::env::var("SEKAI_A10_ICE").as_deref() {
        Ok("double") => IceAlbedoFeedback::ProductionReflectance,
        Ok("recal") => IceAlbedoFeedback::RecalibratedReflectance,
        Ok("preserve") => IceAlbedoFeedback::PlanetaryAlbedoPreserving,
        _ => IceAlbedoFeedback::None,
    };
    // Comma lists of revision physics: lat, seaice, gust (R2); wvar, qdiff (R3).
    let fix_flags = std::env::var("SEKAI_A10_FIX").unwrap_or_default();
    let physics = physics_from_flags(&fix_flags);
    // Reference configuration solved on the same forcing (default: R2).
    let reference_flags =
        std::env::var("SEKAI_A10_REF_FIX").unwrap_or_else(|_| "lat,seaice,gust".to_owned());
    let reference_physics = physics_from_flags(&reference_flags);
    let seed: u64 = std::env::var("SEKAI_A10_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(42);
    // T3b: Anderson depth and mixing of the annual Budyko fixed point.
    let budyko_depth: usize = std::env::var("SEKAI_A10_BUDYKO_DEPTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(
            crate::world::natural::REDUCED_CLIMATE_LAND_EVAPOTRANSPIRATION_ANDERSON_DEPTH as usize,
        );
    let budyko_omega: f64 = std::env::var("SEKAI_A10_BUDYKO_OMEGA")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    let repetitions: usize = std::env::var("SEKAI_A10_REPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(TIMING_REPETITIONS);
    let log = |line: String| {
        eprintln!("[a10 {profile:?} seed {seed} {longwave:?} {ice:?} {physics:?}] {line}")
    };
    let cancellation = BuildCancellation::new();
    let total = Instant::now();

    let started = Instant::now();
    let bundle = ProfileSurfaceBuilder::build(
        profile,
        Meters::new(6_371_000.0).expect("radius"),
        &cancellation,
    )
    .expect("P1");
    let domain_artifact =
        ClimateWorkDomainArtifact::build(bundle.authoritative_surface(), profile, &cancellation)
            .expect("domain");
    let domain = domain_artifact.snapshot();
    log(format!(
        "P1 + climate domain {:.3} s (seed-independent)",
        started.elapsed().as_secs_f64()
    ));
    let started = Instant::now();
    let reduced_grid = ReducedClimateGeometry::new(domain, &|| false).expect("geometry");
    assert_eq!(
        reduced_grid.edges.len(),
        domain_artifact.reduced_climate_geometry().edges.len()
    );
    let grid_seconds = started.elapsed().as_secs_f64();
    log(format!(
        "reduced grid precompute {grid_seconds:.4} s (seed-independent), cells {}",
        reduced_grid.cell_count()
    ));

    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .expect("formation");
    let (tectonic_spec, geologic_spec, relief_spec) = (
        TectonicSpec::default(),
        GeologicSpec::default(),
        ReliefSpec::default(),
    );
    let climate_spec = ClimateSpec::default();
    let surface_spec = HydroErosionSpec::default();
    let mut rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.causal-formation", 5, "sekai.core"),
    ));
    let streams = LabeledSubstreams::capture(&mut rng);
    let surface = bundle.authoritative_surface();
    let evolved = EvolvedTectonicGenerator::generate_from_streams(
        &bundle,
        &tectonic_spec,
        &formation,
        &streams,
    )
    .expect("P2");
    let substrate = GeologicSubstrateGenerator::generate_from_streams(
        surface,
        &evolved,
        &geologic_spec,
        &formation,
        &streams,
    )
    .expect("P3 substrate");
    let (primary_working, relief) = PrimaryReliefGenerator::generate_working_from_streams(
        surface,
        &evolved,
        &substrate,
        &relief_spec,
        &streams,
        &cancellation,
    )
    .expect("P3 relief");
    let state = FormationState::from_primary_working(&primary_working).expect("P5 state");
    let land_mask: Vec<bool> = relief
        .surface_water_geometry()
        .ocean_area_fraction()
        .iter()
        .map(|o| *o < 0.5)
        .collect();
    log(format!(
        "P2+P3 done at {:.1} s",
        total.elapsed().as_secs_f64()
    ));

    // Baseline: production chain.
    let started = Instant::now();
    let start_forcing = GlobalClimateForcingBuilder::build(
        surface,
        &relief,
        substrate.relative_permeability(),
        &climate_spec,
        domain,
        &cancellation,
    )
    .expect("start forcing");
    let start_forcing_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let (baseline_start, start_correction) =
        GlobalCirculationGenerator::generate_capturing_start_correction(
            surface,
            domain,
            &start_forcing,
            ClimateModelProfile::C2LayeredV1,
            &cancellation,
        )
        .expect("baseline start P4");
    let baseline_start_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let closure = SurfaceFormationGenerator::generate_from_exact_state(
        SurfaceFormationInputs {
            surface,
            quality_profile: profile,
            tectonics: &evolved,
            substrate: &substrate,
            relief: &relief,
            domain,
            climate_spec: &climate_spec,
            initial_climate: &baseline_start,
            formation_spec: &surface_spec,
        },
        state.clone(),
        start_correction.as_ref(),
        &cancellation,
    )
    .expect("baseline P5 closure");
    drop(start_correction);
    let baseline_closure_seconds = started.elapsed().as_secs_f64();
    let (baseline_surface, baseline_end, baseline_end_forcing) = closure.into_parts();
    let baseline_terrain = baseline_surface.terrain_fields();
    log(format!(
        "baseline: start forcing {start_forcing_seconds:.3} s, start P4 {baseline_start_seconds:.3} s, P5+endpoint {baseline_closure_seconds:.3} s"
    ));
    if latent_only {
        assert!(
            physics.column_consistent_eddy_moisture
                && physics.nondivergent_zonal_vapour_advection
                && !physics.hadley_meridional_only
                && !physics.zonal_mean_hadley,
            "latent diagnostic supports the final qdiff/zpsi local-Hadley operator"
        );
        let forcing = reduced_forcing(
            &domain_artifact,
            baseline_terrain.current_elevation_m(),
            baseline_terrain.surface_water_geometry(),
            substrate.relative_permeability(),
            &climate_spec,
        );
        let inputs =
            ReducedInputs::from_forcing(&reduced_grid, &forcing, &climate_spec, longwave, ice)
                .with_physics(&reduced_grid, physics)
                .with_budyko_iteration(budyko_depth, budyko_omega);
        let work = solve_reduced_climate(&reduced_grid, &inputs, true);
        let summary = json!({
            "diagnostic_identity": "a10-fixed-flow-latent-r1",
            "profile": format!("{profile:?}"),
            "seed": seed,
            "physics": physics,
            "diagnostics": work.diagnostics,
            "moisture_budget": moisture::budget(&reduced_grid, &inputs, &work),
            "latent_flux": latent::audit(&reduced_grid, &inputs, &work),
            "monthly_storage_scale": latent::monthly_storage_scale(&reduced_grid, &inputs, &work),
            "work_grid_annual": work_grid_dump(&reduced_grid, &inputs, &work),
        });
        let path = out_dir.join(format!("latent-{profile:?}-seed{seed}.json"));
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&summary).expect("diagnostic JSON"),
        )
        .expect("write latent diagnostic");
        log(format!(
            "latent-only diagnostic written to {}",
            path.display()
        ));
        return;
    }
    let baseline_quality = quality::evaluate_global_circulation_quality_for_formation_cancellable(
        surface,
        baseline_terrain,
        &baseline_end_forcing,
        &baseline_end,
        &cancellation,
    )
    .expect("baseline quality");
    let baseline_published = published_from_snapshot(&baseline_end);
    // Baseline re-evaluated with the steering-wind orographic gates for parity.
    let baseline_steering = wrap(
        surface,
        domain,
        &baseline_end_forcing,
        &baseline_published,
        *baseline_end.budget_report(),
    );
    let baseline_steering_quality =
        quality::evaluate_global_circulation_quality_for_formation_cancellable(
            surface,
            baseline_terrain,
            &baseline_end_forcing,
            &baseline_steering,
            &cancellation,
        )
        .expect("baseline steering quality");

    // (a) Reduced model on the baseline endpoint forcing: timing and climate-only comparison.
    let started = Instant::now();
    let mut forcing_times = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        let _ = GlobalClimateForcingBuilder::build_for_formation_terrain(
            surface,
            baseline_terrain,
            substrate.relative_permeability(),
            &climate_spec,
            domain,
            &cancellation,
        )
        .expect("endpoint forcing rebuild");
        forcing_times.push(t.elapsed().as_secs_f64());
    }
    let endpoint_forcing_seconds = median(&mut forcing_times);
    // T3: the production minimal forcing, timed on both terrains and checked against
    // the four quantities the C2 forcing derived the same way.
    let time_reduced = |elevation: &[f32], water: &SurfaceWaterGeometry| {
        let mut seconds = Vec::new();
        let mut forcing = None;
        for _ in 0..5 {
            let t = Instant::now();
            forcing = Some(reduced_forcing(
                &domain_artifact,
                elevation,
                water,
                substrate.relative_permeability(),
                &climate_spec,
            ));
            seconds.push(t.elapsed().as_secs_f64());
        }
        (median(&mut seconds), forcing.expect("five builds"))
    };
    let (reduced_start_forcing_seconds, _) =
        time_reduced(relief.elevation_m(), relief.surface_water_geometry());
    let (reduced_endpoint_forcing_seconds, reduced_endpoint) = time_reduced(
        baseline_terrain.current_elevation_m(),
        baseline_terrain.surface_water_geometry(),
    );
    let widened = |values: &[f32]| values.iter().map(|&v| f64::from(v)).collect::<Vec<_>>();
    assert_eq!(
        reduced_endpoint.land_fraction,
        widened(baseline_end_forcing.planet_forcing().land_fraction())
    );
    assert_eq!(
        reduced_endpoint.land_evapotranspiration_fraction,
        widened(baseline_end_forcing.land_evapotranspiration_fraction())
    );
    assert!(reduced_endpoint
        .monthly_insolation_fraction
        .iter()
        .zip(baseline_end_forcing.monthly_insolation_fraction())
        .all(|(exact, rounded)| exact.map(|v| v as f32) == *rounded));
    assert!(reduced_endpoint
        .cell_publishes_land
        .iter()
        .zip(baseline_end_forcing.source_publishes_land())
        .all(|(&land, &published)| land == (published == 1.0)));
    log(format!(
        "T3 minimal forcing median: start {:.3} ms, endpoint {:.3} ms (C2 endpoint forcing {:.3} ms)",
        reduced_start_forcing_seconds * 1.0e3,
        reduced_endpoint_forcing_seconds * 1.0e3,
        endpoint_forcing_seconds * 1.0e3
    ));
    let _ = started;
    let mut timing = serde_json::Map::new();
    let evaluate = |parallel: bool| {
        let t0 = Instant::now();
        let reduced_forcing_b = reduced_forcing(
            &domain_artifact,
            baseline_terrain.current_elevation_m(),
            baseline_terrain.surface_water_geometry(),
            substrate.relative_permeability(),
            &climate_spec,
        );
        let cell_heights = reduced_forcing_b.cell_reference_height_m.clone();
        let t1 = Instant::now();
        let inputs = ReducedInputs::from_forcing(
            &reduced_grid,
            &reduced_forcing_b,
            &climate_spec,
            longwave,
            ice,
        )
        .with_physics(&reduced_grid, physics)
        .with_budyko_iteration(budyko_depth, budyko_omega);
        let t2 = Instant::now();
        let work = solve_reduced_climate(&reduced_grid, &inputs, parallel);
        let t3 = Instant::now();
        let p5_only = project_for_p5(surface, domain, &reduced_forcing_b, &work, &cancellation);
        let t4 = Instant::now();
        let published = project_all(
            surface,
            domain,
            &reduced_forcing_b,
            &work,
            &cell_heights,
            &cancellation,
        );
        let t5 = Instant::now();
        let budget = budget_report(&reduced_grid, &reduced_forcing_b, &work);
        let snapshot = wrap(surface, domain, &baseline_end_forcing, &published, budget);
        let t6 = Instant::now();
        drop(p5_only);
        let seconds = [
            (t1 - t0).as_secs_f64(),
            (t2 - t1).as_secs_f64(),
            (t3 - t2).as_secs_f64(),
            (t4 - t3).as_secs_f64(),
            (t5 - t4).as_secs_f64(),
            (t6 - t5).as_secs_f64(),
        ];
        (seconds, inputs, work, published, snapshot)
    };
    let labels = [
        "reference_height_remap",
        "inputs_albedo_asr_gradient",
        "work_domain_solve",
        "projection_p5_fields_only",
        "projection_all_fields_lod2",
        "snapshot_wrap_validate_probe_only",
    ];
    let mut serial = vec![Vec::new(); labels.len()];
    let mut parallel = vec![Vec::new(); labels.len()];
    let mut last = None;
    for repetition in 0..repetitions {
        for &is_parallel in &[false, true] {
            let (seconds, inputs, work, published, snapshot) = evaluate(is_parallel);
            let target = if is_parallel {
                &mut parallel
            } else {
                &mut serial
            };
            for (bucket, s) in target.iter_mut().zip(seconds) {
                bucket.push(s);
            }
            if repetition == repetitions - 1 && !is_parallel {
                last = Some((inputs, work, published, snapshot));
            }
        }
    }
    for (label, (s, p)) in labels
        .iter()
        .zip(serial.iter_mut().zip(parallel.iter_mut()))
    {
        timing.insert(
            (*label).to_owned(),
            json!({ "serial_median_s": median(s), "parallel_median_s": median(p), "serial_all": s, "parallel_all": p }),
        );
    }
    timing.insert(
        "endpoint_forcing_build_median_s".into(),
        json!(endpoint_forcing_seconds),
    );
    timing.insert("start_forcing_build_s".into(), json!(start_forcing_seconds));
    timing.insert("reduced_grid_precompute_s".into(), json!(grid_seconds));
    timing.insert(
        "reduced_start_forcing_build_median_s".into(),
        json!(reduced_start_forcing_seconds),
    );
    timing.insert(
        "reduced_endpoint_forcing_build_median_s".into(),
        json!(reduced_endpoint_forcing_seconds),
    );
    timing.insert("baseline_start_p4_s".into(), json!(baseline_start_seconds));
    timing.insert(
        "baseline_p5_plus_endpoint_p4_s".into(),
        json!(baseline_closure_seconds),
    );
    let (inputs_b, work_b, published_b, snapshot_b) = last.expect("timed evaluation");
    let quality_b = quality::evaluate_global_circulation_quality_for_formation_cancellable(
        surface,
        baseline_terrain,
        &baseline_end_forcing,
        &snapshot_b,
        &cancellation,
    )
    .expect("reduced quality on baseline forcing");
    let climate_only = compare_published(surface, &land_mask, &published_b, &baseline_published);
    // The reference configuration (preserving ice, `SEKAI_A10_REF_FIX`) on the
    // same forcing, for the three-way comparison of each design revision.
    let reference_reduced_forcing = reduced_forcing(
        &domain_artifact,
        baseline_terrain.current_elevation_m(),
        baseline_terrain.surface_water_geometry(),
        substrate.relative_permeability(),
        &climate_spec,
    );
    let reference_cells = reference_reduced_forcing.cell_reference_height_m.clone();
    let reference_inputs = ReducedInputs::from_forcing(
        &reduced_grid,
        &reference_reduced_forcing,
        &climate_spec,
        OutgoingLongwaveClosure::BudykoTransEbm,
        IceAlbedoFeedback::PlanetaryAlbedoPreserving,
    )
    .with_physics(&reduced_grid, reference_physics)
    .with_budyko_iteration(budyko_depth, budyko_omega);
    let reference_work = solve_reduced_climate(&reduced_grid, &reference_inputs, true);
    let reference_published = project_all(
        surface,
        domain,
        &reference_reduced_forcing,
        &reference_work,
        &reference_cells,
        &cancellation,
    );
    log(format!(
        "reduced on baseline forcing: {}",
        serde_json::to_string(&work_b.diagnostics).expect("diagnostics JSON")
    ));

    // Scheme-C L0 lapse correction evidence: dry-land neighbour lapse.
    let lapse = neighbour_lapse(
        surface,
        baseline_terrain.current_elevation_m(),
        &land_mask,
        &published_b,
        &baseline_published,
    );

    // Climate-only effect on P5 hydrology (same baseline terrain).
    let hydrology_baseline = FormationHydrologyGenerator::generate(
        surface,
        baseline_terrain,
        &substrate,
        &baseline_end,
        &surface_spec,
        &cancellation,
    )
    .expect("baseline hydrology");
    let hydrology_climate_only = FormationHydrologyGenerator::generate(
        surface,
        baseline_terrain,
        &substrate,
        &snapshot_b,
        &surface_spec,
        &cancellation,
    )
    .expect("reduced-climate hydrology on baseline terrain");

    // (b) Reduced chain: start -> P5 advance -> endpoint.
    let chain = Instant::now();
    let start_reduced_forcing = reduced_forcing(
        &domain_artifact,
        relief.elevation_m(),
        relief.surface_water_geometry(),
        substrate.relative_permeability(),
        &climate_spec,
    );
    let start_cells = start_reduced_forcing.cell_reference_height_m.clone();
    let start_inputs = ReducedInputs::from_forcing(
        &reduced_grid,
        &start_reduced_forcing,
        &climate_spec,
        longwave,
        ice,
    )
    .with_physics(&reduced_grid, physics)
    .with_budyko_iteration(budyko_depth, budyko_omega);
    let start_work = solve_reduced_climate(&reduced_grid, &start_inputs, false);
    let start_published = project_all(
        surface,
        domain,
        &start_reduced_forcing,
        &start_work,
        &start_cells,
        &cancellation,
    );
    let start_snapshot = wrap(
        surface,
        domain,
        &start_forcing,
        &start_published,
        budget_report(&reduced_grid, &start_reduced_forcing, &start_work),
    );
    let start_quality = quality::evaluate_global_circulation_quality(
        surface,
        &relief,
        &start_forcing,
        &start_snapshot,
    )
    .expect("reduced start quality");
    let surface_water =
        SurfaceWaterStencil::new(surface, &cancellation).expect("surface water stencil");
    let mut reduced_state = state.clone();
    let advance = advance_surface_processes(
        &mut reduced_state,
        SurfaceProcessInputs {
            surface,
            surface_water: &surface_water,
            tectonics: &evolved,
            substrate: &substrate,
            climate: &start_snapshot,
            formation_spec: &surface_spec,
            water_inventory_m3: relief.water_inventory_m3(),
        },
        SURFACE_FORMATION_HORIZON_YEARS,
        &cancellation,
    )
    .expect("reduced P5 advance");
    let (_, sediment) = advance.into_parts();
    let reduced_terrain = reduced_state
        .project_final_terrain(
            &surface_water,
            sediment,
            relief.water_inventory_m3(),
            &cancellation,
        )
        .expect("reduced final terrain");
    let end_forcing = GlobalClimateForcingBuilder::build_for_formation_terrain(
        surface,
        &reduced_terrain,
        substrate.relative_permeability(),
        &climate_spec,
        domain,
        &cancellation,
    )
    .expect("reduced endpoint forcing");
    let end_reduced_forcing = reduced_forcing(
        &domain_artifact,
        reduced_terrain.current_elevation_m(),
        reduced_terrain.surface_water_geometry(),
        substrate.relative_permeability(),
        &climate_spec,
    );
    let end_cells = end_reduced_forcing.cell_reference_height_m.clone();
    let end_inputs = ReducedInputs::from_forcing(
        &reduced_grid,
        &end_reduced_forcing,
        &climate_spec,
        longwave,
        ice,
    )
    .with_physics(&reduced_grid, physics)
    .with_budyko_iteration(budyko_depth, budyko_omega);
    let end_work = solve_reduced_climate(&reduced_grid, &end_inputs, false);
    // T3b: the production pair of evaluations, start cold and endpoint warm-started from
    // the start's converged Budyko fractions, both parallel; five-run medians.
    let warm_end_inputs = ReducedInputs {
        initial_land_evaporative_fraction: Some(start_work.land_evaporative_fraction.clone()),
        ..end_inputs.clone()
    };
    let pair_timer = Instant::now();
    let mut pair_start = Vec::new();
    let mut pair_end = Vec::new();
    let mut pair_rounds = (0, 0);
    for _ in 0..repetitions {
        let t = Instant::now();
        let cold = solve_reduced_climate(&reduced_grid, &start_inputs, true);
        pair_start.push(t.elapsed().as_secs_f64());
        let t = Instant::now();
        let warm = solve_reduced_climate(&reduced_grid, &warm_end_inputs, true);
        pair_end.push(t.elapsed().as_secs_f64());
        pair_rounds = (
            cold.diagnostics.land_evapotranspiration_iterations,
            warm.diagnostics.land_evapotranspiration_iterations,
        );
    }
    let pair_seconds = pair_timer.elapsed().as_secs_f64();
    let production_pair = json!({
        "start_cold_parallel_median_s": median(&mut pair_start),
        "endpoint_warm_parallel_median_s": median(&mut pair_end),
        "start_budyko_rounds": pair_rounds.0,
        "endpoint_budyko_rounds": pair_rounds.1,
    });
    let end_published = project_all(
        surface,
        domain,
        &end_reduced_forcing,
        &end_work,
        &end_cells,
        &cancellation,
    );
    let end_snapshot = wrap(
        surface,
        domain,
        &end_forcing,
        &end_published,
        budget_report(&reduced_grid, &end_reduced_forcing, &end_work),
    );
    let chain_seconds = chain.elapsed().as_secs_f64() - pair_seconds;
    let end_quality = quality::evaluate_global_circulation_quality_for_formation_cancellable(
        surface,
        &reduced_terrain,
        &end_forcing,
        &end_snapshot,
        &cancellation,
    )
    .expect("reduced endpoint quality");
    let hydrology_chain = FormationHydrologyGenerator::generate(
        surface,
        &reduced_terrain,
        &substrate,
        &end_snapshot,
        &surface_spec,
        &cancellation,
    )
    .expect("reduced chain hydrology");
    let areas: Vec<f64> = surface.cells().iter().map(|c| c.area.get()).collect();
    let (mut sq, mut max_abs, mut w) = (0.0_f64, 0.0_f64, 0.0_f64);
    for ((a, b), area) in reduced_terrain
        .current_elevation_m()
        .iter()
        .zip(baseline_terrain.current_elevation_m())
        .zip(&areas)
    {
        let d = f64::from(a - b);
        sq += area * d * d;
        w += area;
        max_abs = max_abs.max(d.abs());
    }
    let start_compare = compare_published(
        surface,
        &land_mask,
        &start_published,
        &published_from_snapshot(&baseline_start),
    );

    // R7: only the requested audit pays for these same-forcing comparisons.
    // qcolumn is an ablation, not a replacement justified by Siler's MEBM.
    let mut moisture_experiments = serde_json::Map::new();
    if std::env::var("SEKAI_A10_MOISTURE_AUDIT").is_ok() {
        for name in ["hmean", "qcolumn"] {
            let mut trial_inputs = inputs_b.clone();
            match name {
                "hmean" => trial_inputs.physics.zonal_mean_hadley = true,
                "qcolumn" => trial_inputs.physics.column_consistent_eddy_moisture = false,
                _ => unreachable!("audit axes are fixed"),
            }
            let trial_work = solve_reduced_climate(&reduced_grid, &trial_inputs, true);
            assert_eq!(
                trial_work.air_temperature_c, work_b.air_temperature_c,
                "water-only audit changed E1 temperature"
            );
            let trial_published = project_all(
                surface,
                domain,
                &reference_reduced_forcing,
                &trial_work,
                &reference_cells,
                &cancellation,
            );
            moisture_experiments.insert(name.to_owned(), json!({
                "budget": moisture::budget(&reduced_grid, &trial_inputs, &trial_work),
                "diagnostics": trial_work.diagnostics,
                "t3b": t3b_metrics(surface, domain, &land_mask, &trial_published,
                    Some((&reduced_grid, &trial_inputs, &trial_work)), substrate.relative_permeability()),
                "temperature": temperature_summary(surface, &trial_published),
                "zonal": zonal_profiles(surface, &trial_published),
                "work_grid_annual": (seed == 42).then(|| work_grid_dump(&reduced_grid, &trial_inputs, &trial_work)),
            }));
        }
    }

    let summary = json!({
        "profile": format!("{profile:?}"),
        "longwave_closure": longwave,
        "ice_albedo_feedback": ice,
        "temperature_summary": {
            "reduced_on_baseline_forcing": temperature_summary(surface, &published_b),
            "baseline_endpoint": temperature_summary(surface, &baseline_published),
            "reduced_chain_end": temperature_summary(surface, &end_published),
            "reference_on_baseline_forcing": temperature_summary(surface, &reference_published),
        },
        "zonal_profiles": {
            "band_degrees": ZONAL_BAND_DEGREES,
            "reduced_on_baseline_forcing": zonal_profiles(surface, &published_b),
            "baseline_endpoint": zonal_profiles(surface, &baseline_published),
            "reference_on_baseline_forcing": zonal_profiles(surface, &reference_published),
        },
        "reference_on_baseline_forcing": {
            "diagnostics": reference_work.diagnostics,
        },
        "moisture_audit": {
            "selected": moisture::budget(&reduced_grid, &inputs_b, &work_b),
            "reference": moisture::budget(&reduced_grid, &reference_inputs, &reference_work),
            "experiments": moisture_experiments,
            "reference_work_grid_annual": (seed == 42)
                .then(|| work_grid_dump(&reduced_grid, &reference_inputs, &reference_work)),
        },
        "physics": physics,
        "reference_physics": reference_physics,
        "work_grid_annual": work_grid_dump(&reduced_grid, &inputs_b, &work_b),
        "seed": seed,
        "authoritative_cells": surface.cells().len(),
        "work_cells": reduced_grid.cell_count(),
        "vapour_column_mass_kg_m2": vapour_column_mass_kg_m2(),
        "timing": Value::Object(timing),
        "chain_seconds_including_p5_and_two_reduced_solves": chain_seconds,
        "production_pair_solve": production_pair,
        "t3b": {
            "reduced": t3b_metrics(surface, domain, &land_mask, &published_b, Some((&reduced_grid, &inputs_b, &work_b)), substrate.relative_permeability()),
            "reference": t3b_metrics(surface, domain, &land_mask, &reference_published, Some((&reduced_grid, &reference_inputs, &reference_work)), substrate.relative_permeability()),
            "baseline_p4": t3b_metrics(surface, domain, &land_mask, &baseline_published, None, substrate.relative_permeability()),
        },
        "reduced_on_baseline_forcing": {
            "diagnostics": work_b.diagnostics,
            "budget": snapshot_b.budget_report(),
            "quality": metrics_json(&quality_b),
            "climate_only_vs_baseline_endpoint": climate_only,
            "dry_land_neighbour_lapse": lapse,
            "hydrology_climate_only_vs_baseline": compare_hydrology(surface, &hydrology_climate_only, &hydrology_baseline),
            "albedo_global_mean_work": inputs_b.surface_albedo.iter().zip(&reduced_grid.area_m2).map(|(a, w)| a * w).sum::<f64>()
                / reduced_grid.area_m2.iter().sum::<f64>(),
        },
        "baseline": {
            "quality": metrics_json(&baseline_quality),
            "quality_with_steering_wind_orography": metrics_json(&baseline_steering_quality),
            "budget": baseline_end.budget_report(),
        },
        "reduced_chain": {
            "start_diagnostics": start_work.diagnostics,
            "end_diagnostics": end_work.diagnostics,
            "start_quality_vs_relief": metrics_json(&start_quality),
            "end_quality": metrics_json(&end_quality),
            "start_vs_baseline_start": start_compare,
            "final_elevation_rms_difference_m": (sq / w).sqrt(),
            "final_elevation_max_abs_difference_m": max_abs,
            "hydrology_vs_baseline": compare_hydrology(surface, &hydrology_chain, &hydrology_baseline),
        },
        "total_probe_seconds": total.elapsed().as_secs_f64(),
    });
    let fix = if fix_flags.is_empty() {
        "none".to_owned()
    } else {
        fix_flags.replace(',', "+")
    };
    let path = out_dir.join(format!(
        "probe-{}-{}-ice{}-fix{fix}-seed{seed}.json",
        format!("{profile:?}").to_lowercase(),
        format!("{longwave:?}").to_lowercase(),
        format!("{ice:?}").to_lowercase()
    ));
    std::fs::write(&path, serde_json::to_string_pretty(&summary).expect("json"))
        .expect("write summary");
    if std::env::var("SEKAI_A10_DUMP_CELLS").is_ok() {
        let terrain_sea = baseline_terrain.surface_water_geometry().sea_level_m();
        let cells = json!({
            "note": "authoritative cells; baseline endpoint terrain; annual means",
            "lat_deg": surface.cells().iter().map(|c| c.centroid.components()[2].asin().to_degrees()).collect::<Vec<_>>(),
            "lon_deg": surface.cells().iter().map(|c| { let v = c.centroid.components(); v[1].atan2(v[0]).to_degrees() }).collect::<Vec<_>>(),
            "elevation_above_sea_m": baseline_terrain.current_elevation_m().iter().map(|&z| f64::from(z - terrain_sea)).collect::<Vec<_>>(),
            "ocean_fraction": baseline_terrain.surface_water_geometry().ocean_area_fraction(),
            "edges": surface
                .edges()
                .iter()
                .map(|edge| [edge.cells[0].raw(), edge.cells[1].raw()])
                .collect::<Vec<_>>(),
            "old_p4": cell_dump(surface, &baseline_published),
            "reference": cell_dump(surface, &reference_published),
            "r2": cell_dump(surface, &published_b),
        });
        let cell_path = out_dir.join(format!(
            "cells-{}-seed{seed}-fix{fix}.json",
            format!("{profile:?}").to_lowercase()
        ));
        std::fs::write(&cell_path, serde_json::to_string(&cells).expect("json"))
            .expect("write cells");
    }
    log(format!("wrote {}", path.display()));
}

/// Dry-land neighbour effective lapse rate (OLS dT/dz) and zero-dT share among
/// |dz| >= 300 m pairs, as in the LOD-2 audit.
/// T3b evidence (design section 18.3-3) on the authoritative surface: area-weighted annual
/// precipitation of land and ocean cells, and the land runoff P5 would route, both from the
/// published climate (`P (1 - eps)` with the work-grid evaporative fraction projected
/// intensively, through the production P5 land-runoff contract) and from the work-grid
/// ledger `sum A f_land (1 - eps) P`. The baseline row routes the A3 permeability share.
fn t3b_metrics(
    surface: &SphericalSurfaceSnapshot,
    domain: &ClimateWorkDomainSnapshot,
    land_mask: &[bool],
    published: &Published,
    climate: Option<(&ReducedClimateGeometry, &ReducedInputs, &ReducedWorkClimate)>,
    relative_permeability: &[f32],
) -> Value {
    let annual_mm_day =
        |m: &[f32; MONTHS]| m.iter().map(|&v| f64::from(v)).sum::<f64>() / MONTHS as f64;
    let (mut land_p, mut land_a, mut ocean_p, mut ocean_a) = (0.0, 0.0, 0.0, 0.0);
    for (cell, months) in published.precipitation.iter().enumerate() {
        let area = surface.cells()[cell].area.get();
        if land_mask[cell] {
            land_p += area * annual_mm_day(months);
            land_a += area;
        } else {
            ocean_p += area * annual_mm_day(months);
            ocean_a += area;
        }
    }
    let km3 = |mm_area: f64| mm_area * 1.0e-3 * 1.0e-9;
    let fraction = climate.map(|(_, _, work)| {
        let work_fraction: Vec<f32> = work
            .land_evaporative_fraction
            .iter()
            .map(|&v| v as f32)
            .collect();
        project_intensive_scalar_cancellable(domain, &work_fraction, &BuildCancellation::new())
            .expect("evaporative fraction projection")
    });
    let mut routed_mm_area = 0.0;
    for (cell, months) in published.precipitation.iter().enumerate() {
        if !land_mask[cell] {
            continue;
        }
        let area = surface.cells()[cell].area.get();
        let annual: f64 = match &fraction {
            Some(fraction) => {
                let runoff: [f32; MONTHS] =
                    std::array::from_fn(|m| months[m] * (1.0 - fraction[cell]).clamp(0.0, 1.0));
                formation_monthly_land_runoff_mm(months, &runoff)
                    .iter()
                    .sum()
            }
            None => {
                formation_monthly_precipitation_mm(months)
                    .iter()
                    .sum::<f64>()
                    * formation_runoff_fraction(relative_permeability[cell])
            }
        };
        routed_mm_area += area * annual;
    }
    let ledger_km3 = climate.map(|(grid, inputs, work)| {
        (0..grid.cell_count())
            .map(|c| {
                grid.area_m2[c]
                    * inputs.land_fraction[c]
                    * (1.0 - work.land_evaporative_fraction[c])
                    * annual_mm_day(&work.precipitation_mm_day[c].map(|v| v as f32))
                    * 365.2422
            })
            .sum::<f64>()
            * 1.0e-12
    });
    json!({
        "land_cell_precipitation_mm_day": land_p / land_a,
        "ocean_cell_precipitation_mm_day": ocean_p / ocean_a,
        "p5_routed_land_runoff_km3_per_year": km3(routed_mm_area),
        "work_ledger_land_runoff_km3_per_year": ledger_km3,
    })
}

fn neighbour_lapse(
    surface: &SphericalSurfaceSnapshot,
    elevation: &[f32],
    land: &[bool],
    reduced: &Published,
    baseline: &Published,
) -> Value {
    let stats = |p: &Published| {
        let (mut sxy, mut sxx, mut big, mut zero) = (0.0_f64, 0.0_f64, 0usize, 0usize);
        for edge in surface.edges() {
            let (a, b) = (edge.cells[0].raw() as usize, edge.cells[1].raw() as usize);
            if !(land[a] && land[b]) {
                continue;
            }
            let dz = f64::from(elevation[a] - elevation[b]) / 1000.0;
            let dt = annual(&p.air[a]) - annual(&p.air[b]);
            sxy += dz * dt;
            sxx += dz * dz;
            if dz.abs() >= 0.3 {
                big += 1;
                zero += usize::from(dt == 0.0);
            }
        }
        json!({ "ols_k_per_km": sxy / sxx, "large_dz_pairs": big, "zero_dt_pairs": zero })
    };
    json!({ "reduced_lod2_c": stats(reduced), "baseline_current_p4": stats(baseline) })
}

/// Annual work-grid fields for offline pattern analysis.
fn work_grid_dump(
    grid: &ReducedClimateGeometry,
    inputs: &ReducedInputs,
    work: &ReducedWorkClimate,
) -> Value {
    let mean = |m: &[f64; MONTHS]| m.iter().sum::<f64>() / MONTHS as f64;
    let mean_vector = |m: &[[f64; 3]; MONTHS]| {
        let mut s = [0.0; 3];
        for v in m {
            for c in 0..3 {
                s[c] += v[c] / MONTHS as f64;
            }
        }
        s
    };
    let rows: Vec<Value> = (0..grid.cell_count())
        .map(|c| {
            let east = grid.east[c];
            let north = super::cross(grid.center[c], east);
            let wind = mean_vector(&work.wind_m_s[c]);
            let current = mean_vector(&work.ocean_current_m_s[c]);
            json!([
                grid.latitude_rad[c].to_degrees(),
                grid.center[c][1].atan2(grid.center[c][0]).to_degrees(),
                inputs.land_fraction[c],
                inputs.reference_height_m[c],
                mean(&work.air_temperature_c[c]),
                work.air_temperature_c[c][6] - work.air_temperature_c[c][0],
                mean(&work.precipitation_mm_day[c]),
                mean(&work.evaporation_mm_day[c]),
                dot(wind, east),
                dot(wind, north),
                dot(current, east),
                dot(current, north),
                mean(&work.sea_surface_height_m[c]),
                work.ice_fraction[c],
                mean(&work.monthly_absorbed_shortwave_w_m2[c]),
                mean(&work.outgoing_longwave_w_m2[c]),
                inputs.sea_ice[c],
                inputs.land_evapotranspiration_fraction[c],
                work.vapour_convergence_mm_day[c][0],
                work.vapour_convergence_mm_day[c][1],
                work.vapour_convergence_mm_day[c][2],
                (0..MONTHS)
                    .map(|m| {
                        work.specific_humidity[c][m]
                            / saturation_specific_humidity_kg_kg(work.theta_c[c][m])
                    })
                    .sum::<f64>()
                    / MONTHS as f64,
                work.land_evaporative_fraction[c],
                work.land_potential_evaporation_mm_day[c],
                grid.area_m2[c],
                (0..MONTHS)
                    .map(|m| work.specific_humidity[c][m]
                        / saturation_specific_humidity_kg_kg(work.air_temperature_c[c][m]))
                    .sum::<f64>()
                    / MONTHS as f64,
            ])
        })
        .collect();
    json!({
        "columns": ["lat", "lon", "land", "ref_height_m", "air_c", "air_jul_minus_jan_c",
            "p_mm_day", "e_mm_day", "u_east", "v_north", "current_east", "current_north", "ssh_m", "ice_fraction",
            "asr_w_m2", "olr_w_m2", "sea_ice", "beta", "conv_eddy", "conv_hadley", "conv_zonal", "rh_sea_level",
            "land_evaporative_fraction", "land_potential_mm_day", "area_m2", "rh_reference_height"],
        "rows": rows,
        // Work-grid adjacency for grid-scale (2-delta-x) diagnostics:
        // [cell 0, cell 1, east component of the unit normal].
        "edges": grid
            .edges
            .iter()
            .map(|e| json!([e.cells[0], e.cells[1], dot(super::east_unit(e.midpoint), e.normal)]))
            .collect::<Vec<_>>(),
    })
}

/// Global, polar-cap (|lat| >= 60) and tropical annual air temperature.
fn temperature_summary(surface: &SphericalSurfaceSnapshot, published: &Published) -> Value {
    let mut sums = [0.0_f64; 4];
    let mut weights = [0.0_f64; 4];
    for (cell, air) in surface.cells().iter().zip(&published.air) {
        let latitude = cell.centroid.components()[2].asin().to_degrees();
        let value = annual(air);
        let area = cell.area.get();
        let mut add = |slot: usize| {
            sums[slot] += area * value;
            weights[slot] += area;
        };
        add(0);
        if latitude <= -60.0 {
            add(1);
        }
        if latitude >= 60.0 {
            add(2);
        }
        if latitude.abs() <= 10.0 {
            add(3);
        }
    }
    let mean = |slot: usize| sums[slot] / weights[slot].max(f64::MIN_POSITIVE);
    json!({
        "global_mean_c": mean(0),
        "south_cap_60_90_c": mean(1),
        "north_cap_60_90_c": mean(2),
        "tropics_10s_10n_c": mean(3),
    })
}

/// Area-weighted zonal bands of annual air temperature, July minus January
/// air temperature, precipitation and evaporation on the authoritative cells.
fn zonal_profiles(surface: &SphericalSurfaceSnapshot, published: &Published) -> Value {
    let bands = (180.0 / ZONAL_BAND_DEGREES) as usize;
    let mut sums = vec![[0.0_f64; 4]; bands];
    let mut weights = vec![0.0_f64; bands];
    for (index, cell) in surface.cells().iter().enumerate() {
        let latitude = cell.centroid.components()[2].asin().to_degrees();
        let band = (((latitude + 90.0) / ZONAL_BAND_DEGREES) as usize).min(bands - 1);
        let area = cell.area.get();
        let values = [
            annual(&published.air[index]),
            f64::from(published.air[index][6] - published.air[index][0]),
            annual(&published.precipitation[index]),
            annual(&published.evaporation[index]),
        ];
        for (sum, value) in sums[band].iter_mut().zip(values) {
            *sum += area * value;
        }
        weights[band] += area;
    }
    let column = |slot: usize| -> Vec<f64> {
        sums.iter()
            .zip(&weights)
            .map(|(s, w)| s[slot] / w.max(f64::MIN_POSITIVE))
            .collect()
    };
    let centres: Vec<f64> = (0..bands)
        .map(|b| -90.0 + (b as f64 + 0.5) * ZONAL_BAND_DEGREES)
        .collect();
    json!({
        "latitude_centre_deg": centres,
        "air_c": column(0),
        "air_jul_minus_jan_c": column(1),
        "precipitation_mm_day": column(2),
        "evaporation_mm_day": column(3),
    })
}

/// Annual air temperature, precipitation and east/north mean wind per cell.
fn cell_dump(surface: &SphericalSurfaceSnapshot, published: &Published) -> Value {
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    let mut east_wind = Vec::with_capacity(published.wind.len());
    let mut north_wind = Vec::with_capacity(published.wind.len());
    for (cell, wind) in surface.cells().iter().zip(&published.wind) {
        let mean = annual_vector(wind);
        let (east, north) = canonical_east_north_basis(cell.centroid);
        east_wind.push(round(dot(east, mean)));
        north_wind.push(round(dot(north, mean)));
    }
    json!({
        "air_c": published.air.iter().map(|m| round(annual(m))).collect::<Vec<_>>(),
        "air_jan_c": published.air.iter().map(|m| round(f64::from(m[0]))).collect::<Vec<_>>(),
        "air_jul_c": published.air.iter().map(|m| round(f64::from(m[6]))).collect::<Vec<_>>(),
        "precipitation_mm_day": published.precipitation.iter().map(|m| round(annual(m))).collect::<Vec<_>>(),
        "wind_east_m_s": east_wind,
        "wind_north_m_s": north_wind,
    })
}

/// Parses a comma list of revision physics flags.
fn physics_from_flags(flags: &str) -> ReducedPhysics {
    let has = |flag: &str| flags.split(',').any(|f| f == flag);
    ReducedPhysics {
        latitude_albedo: has("lat"),
        sea_ice_surface_type: has("seaice"),
        evaporation_gustiness: has("gust"),
        evaporation_wind_variability: has("wvar"),
        column_consistent_eddy_moisture: has("qdiff"),
        land_potential_evaporation_cap: has("landcap"),
        budyko_land_evapotranspiration: has("bdk"),
        thornthwaite_potential_evaporation: has("thw"),
        hadley_meridional_only: has("hmer"),
        zonal_mean_hadley: has("hmean"),
        band_mean_zonal_vapour_advection: has("zband"),
        nondivergent_zonal_vapour_advection: has("zpsi"),
    }
}
