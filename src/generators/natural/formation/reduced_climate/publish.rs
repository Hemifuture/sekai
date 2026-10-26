//! Validation-only wiring of the A10 reduced climate into the production
//! chain (branch `a10-climate-ui-preview`, never merged). It publishes the
//! reduced fields in the C2-shaped snapshot the current consumers read, with
//! placeholder vertical fields and the existing identities; see the A10
//! design, revisions R1 and R2, for what this preview does and does not
//! provide. It runs the R4 physics: R2 latitude albedo and sea-ice surface
//! type, R3 observed wind variability in evaporation and column-consistent
//! eddy vapour diffusion, and the R4 streamfunction (non-divergent) zonal
//! vapour advection.

use super::{
    solve_reduced_climate, IceAlbedoFeedback, OutgoingLongwaveClosure, ReducedGrid, ReducedInputs,
    ReducedPhysics, ReducedWorkClimate,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::CubedSphereGrid;
use crate::generators::natural::formation::global_circulation::{
    global_circulation_model_fingerprint, project_intensive_scalar_cancellable,
    project_monthly_extensive_rate_cancellable, project_monthly_intensive_scalar_cancellable,
    project_monthly_tangent_vectors_cancellable, GlobalClimateForcing,
    SELECTED_PRODUCTION_INTEGRATOR,
};
use crate::generators::spatial::remap_intensive_f32;
use crate::world::natural::{
    atmospheric_reference_surface_height_m, expected_global_circulation_dense_state_bytes,
    saturation_specific_humidity_kg_kg, ClimateBudgetReport, ClimateCapabilitySet,
    ClimateCheckpoint, ClimateLayerLayout, ClimateModelProfile, ClimateQuantizationId,
    ClimateRemapReport, ClimateSolveReport, ClimateSpec, ClimateWorkDomainSnapshot,
    GlobalCirculationFields, GlobalCirculationSnapshot, MonthlyScalarField, MonthlyVector3Field,
    SurfaceWaterGeometry, CLIMATE_MONTH_COUNT, CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M,
    EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2,
    GLOBAL_CIRCULATION_MINIMUM_FAST_SUBSTEPS_PER_MACRO_STEP, GLOBAL_CIRCULATION_SCHEMA_V2,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};

/// Reduced climate snapshot for one terrain (start relief or P5 endpoint).
pub(in crate::generators::natural) fn generate_reduced_snapshot(
    surface: &SphericalSurfaceSnapshot,
    domain: &ClimateWorkDomainSnapshot,
    forcing: &GlobalClimateForcing,
    spec: &ClimateSpec,
    terrain: (&[f32], &SurfaceWaterGeometry),
    cancellation: &BuildCancellation,
) -> GlobalCirculationSnapshot {
    let grid = CubedSphereGrid::new(
        domain.face_resolution(),
        domain.climate_surface().radius().get(),
    )
    .expect("climate work grid");
    let grid = ReducedGrid::new(&grid);
    let (cell_heights, work_heights) = reference_heights(domain, terrain.0, terrain.1);
    let inputs = ReducedInputs::from_forcing(
        &grid,
        forcing,
        spec,
        work_heights,
        OutgoingLongwaveClosure::BudykoTransEbm,
        IceAlbedoFeedback::PlanetaryAlbedoPreserving,
    )
    .with_physics(
        &grid,
        ReducedPhysics {
            latitude_albedo: true,
            sea_ice_surface_type: true,
            evaporation_gustiness: false,
            evaporation_wind_variability: true,
            column_consistent_eddy_moisture: true,
            land_potential_evaporation_cap: false,
            hadley_meridional_only: false,
            band_mean_zonal_vapour_advection: false,
            nondivergent_zonal_vapour_advection: true,
        },
    );
    let work = solve_reduced_climate(&grid, &inputs, true);
    let published = project_all(surface, domain, forcing, &work, &cell_heights, cancellation);
    let budget = budget_report(&grid, forcing, &work);
    wrap(surface, domain, forcing, &published, budget)
}

const MONTHS: usize = CLIMATE_MONTH_COUNT;
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
fn reference_heights(
    domain: &ClimateWorkDomainSnapshot,
    elevation_m: &[f32],
    water: &SurfaceWaterGeometry,
) -> (Vec<f64>, Vec<f64>) {
    let sea = water.sea_level_m();
    let h_c: Vec<f32> = elevation_m
        .iter()
        .zip(water.ocean_area_fraction())
        .map(|(&z, &ocean)| atmospheric_reference_surface_height_m(z - sea, 1.0 - ocean) as f32)
        .collect();
    let work =
        remap_intensive_f32(domain.source_to_climate(), &h_c).expect("reference height remap");
    (
        h_c.iter().map(|&v| f64::from(v)).collect(),
        work.iter().map(|&v| f64::from(v)).collect(),
    )
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
    forcing: &GlobalClimateForcing,
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
    forcing: &GlobalClimateForcing,
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
    for (vectors, &land) in current.iter_mut().zip(forcing.source_publishes_land()) {
        if land == 1.0 {
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

/// Global budget numbers on the work grid (production `final_cycle_budget`
/// semantics: area-month means, CERES planetary albedo of the surface albedo).
fn budget_report(
    grid: &ReducedGrid,
    forcing: &GlobalClimateForcing,
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
                * f64::from(forcing.monthly_insolation_fraction()[cell][month]);
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
        SurfaceRef::from_validated_spherical(surface).expect("surface ref"),
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
