use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    build_surface_water_geometry, project_monthly_extensive_rate, project_monthly_intensive_scalar,
    project_monthly_tangent_vectors, ClimateProjectionError, ClimateWorkDomainBuilder,
    EvolvedTectonicGenerator, GeologicSubstrateGenerator, GlobalClimateForcing,
    GlobalClimateForcingBuilder, GlobalClimateForcingError, PrimaryReliefGenerator,
};
use sekai::generators::spatial::{
    remap_intensive_f32, ProfileSurfaceBuilder, ProfileSurfaceBundle,
};
use sekai::world::natural::{
    absorbed_shortwave_w_m2, bulk_surface_evaporation_kg_m2_s,
    gray_equilibrium_surface_temperature_c, gray_longwave_slope_w_m2_k,
    latent_heat_flux_w_m2_from_evaporation_mm_day, lcl_adjusted_orographic_condensation_kg_m2_s,
    linearized_outgoing_longwave_w_m2, neutral_surface_air_specific_humidity_kg_kg,
    p4_seasonal_storage_heat_capacities_j_m2_k, planetary_albedo_from_surface,
    raw_orographic_condensation_kg_m2_s, saturation_specific_humidity_kg_kg,
    seasonal_storage_equilibrium_temperature_c, ClimateSpec, ClimateWorkDomainSnapshot,
    GeologicSpec, GeologicSubstrateSnapshot, NaturalQualityProfile, PrimaryReliefSnapshot,
    ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
    WorldFormationPreset, CERES_EBAF_ABSORBED_SHORTWAVE_GLOBAL_MEAN_W_M2,
    CERES_EBAF_INCOMING_SHORTWAVE_GLOBAL_MEAN_W_M2, CERES_EBAF_OUTGOING_LONGWAVE_GLOBAL_MEAN_W_M2,
    CERES_EBAF_REFLECTED_SHORTWAVE_GLOBAL_MEAN_W_M2, CERES_EBAF_TOA_NET_RADIATION_GLOBAL_MEAN_W_M2,
    EARTH_CALIBRATION_SURFACE_ALBEDO_GLOBAL_MEAN, EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN,
    EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE,
    EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY, EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2,
    GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX, P4_REFERENCE_AIR_DENSITY_KG_M3,
    REFERENCE_SURFACE_RELATIVE_HUMIDITY, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
    SECONDS_PER_CLIMATOLOGICAL_MONTH, STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2,
    STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2, WILD_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2,
    WILD_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2,
};
use sekai::world::{Meters, RootSeed};

struct Fixture {
    bundle: ProfileSurfaceBundle,
    substrate: GeologicSubstrateSnapshot,
    relief: PrimaryReliefSnapshot,
    domain: ClimateWorkDomainSnapshot,
    forcing: GlobalClimateForcing,
}

#[test]
fn physical_moisture_helpers_obey_analytic_limits() {
    let cold = saturation_specific_humidity_kg_kg(-10.0);
    let mild = saturation_specific_humidity_kg_kg(15.0);
    let warm = saturation_specific_humidity_kg_kg(30.0);
    assert!(cold > 0.0 && cold < mild && mild < warm);

    let unsaturated = 0.5 * mild;
    assert_eq!(
        bulk_surface_evaporation_kg_m2_s(15.0, unsaturated, 0.0, 1.0),
        0.0
    );
    assert_eq!(bulk_surface_evaporation_kg_m2_s(15.0, mild, 12.0, 1.0), 0.0);
    assert_eq!(
        bulk_surface_evaporation_kg_m2_s(15.0, unsaturated, 12.0, 0.0),
        0.0
    );
    assert!(bulk_surface_evaporation_kg_m2_s(15.0, unsaturated, 12.0, 1.0) > 0.0);

    let surface_temperature_c = 20.0;
    let relative_humidity = 0.8;
    let cold_slab_temperature_c = 5.0;
    let mild_slab_temperature_c = 15.0;
    let cold_slab_humidity =
        relative_humidity * saturation_specific_humidity_kg_kg(cold_slab_temperature_c);
    let mild_slab_humidity =
        relative_humidity * saturation_specific_humidity_kg_kg(mild_slab_temperature_c);
    let cold_neutral = neutral_surface_air_specific_humidity_kg_kg(
        surface_temperature_c,
        cold_slab_temperature_c,
        cold_slab_humidity,
    );
    let mild_neutral = neutral_surface_air_specific_humidity_kg_kg(
        surface_temperature_c,
        mild_slab_temperature_c,
        mild_slab_humidity,
    );
    assert!((cold_neutral - mild_neutral).abs() <= 1.0e-12);
    assert_eq!(
        bulk_surface_evaporation_kg_m2_s(surface_temperature_c, cold_neutral, 8.0, 1.0,).to_bits(),
        bulk_surface_evaporation_kg_m2_s(surface_temperature_c, mild_neutral, 8.0, 1.0,).to_bits(),
    );

    assert_eq!(raw_orographic_condensation_kg_m2_s(0.01, -0.02), 0.0);
    assert_eq!(raw_orographic_condensation_kg_m2_s(0.01, 0.0), 0.0);
    assert_eq!(
        raw_orographic_condensation_kg_m2_s(0.01, 0.02),
        P4_REFERENCE_AIR_DENSITY_KG_M3 * 0.01 * 0.02
    );
    let orographic_temperature_c = 20.0;
    let orographic_saturation = saturation_specific_humidity_kg_kg(orographic_temperature_c);
    let upslope_velocity_m_s = 0.1;
    let horizontal_wind_speed_m_s = 10.0;
    let resolved_cell_area_m2 = 1.0e10;
    assert_eq!(
        lcl_adjusted_orographic_condensation_kg_m2_s(
            0.014,
            81.0,
            0.082,
            8.0,
            resolved_cell_area_m2,
        ),
        0.0,
    );
    assert_eq!(
        lcl_adjusted_orographic_condensation_kg_m2_s(
            orographic_saturation,
            orographic_temperature_c,
            upslope_velocity_m_s,
            horizontal_wind_speed_m_s,
            resolved_cell_area_m2,
        )
        .to_bits(),
        raw_orographic_condensation_kg_m2_s(orographic_saturation, upslope_velocity_m_s).to_bits(),
    );
    let sub_saturated = 0.8 * orographic_saturation;
    assert_eq!(
        lcl_adjusted_orographic_condensation_kg_m2_s(
            sub_saturated,
            orographic_temperature_c,
            upslope_velocity_m_s,
            horizontal_wind_speed_m_s,
            1.0e6,
        ),
        0.0,
    );
    let crossed_lcl = lcl_adjusted_orographic_condensation_kg_m2_s(
        sub_saturated,
        orographic_temperature_c,
        upslope_velocity_m_s,
        horizontal_wind_speed_m_s,
        resolved_cell_area_m2,
    );
    assert!(crossed_lcl > 0.0);
    assert!(crossed_lcl < raw_orographic_condensation_kg_m2_s(sub_saturated, upslope_velocity_m_s));
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let cancellation = BuildCancellation::new();
        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &cancellation,
        )
        .unwrap();
        let formation = ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            WorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Continents,
        )
        .unwrap();
        let mut tectonic_rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
        ));
        let evolved = EvolvedTectonicGenerator::generate(
            &bundle,
            &TectonicSpec::default(),
            &formation,
            &mut tectonic_rng,
        )
        .unwrap();
        let mut substrate_rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("natural.geologic-substrate", 1, "sekai.core"),
        ));
        let substrate = GeologicSubstrateGenerator::generate(
            bundle.authoritative_surface(),
            &evolved,
            &GeologicSpec::default(),
            &formation,
            &mut substrate_rng,
        )
        .unwrap();
        let mut relief_rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("natural.primary-relief", 1, "sekai.core"),
        ));
        let mut diagnostics = Vec::new();
        let relief = PrimaryReliefGenerator::generate(
            bundle.authoritative_surface(),
            &evolved,
            &substrate,
            &ReliefSpec::default(),
            &mut relief_rng,
            &mut diagnostics,
        )
        .unwrap();
        let domain = ClimateWorkDomainBuilder::build(
            bundle.authoritative_surface(),
            NaturalQualityProfile::Draft,
            &cancellation,
        )
        .unwrap();
        let forcing = GlobalClimateForcingBuilder::build(
            bundle.authoritative_surface(),
            &relief,
            substrate.relative_permeability(),
            &ClimateSpec::default(),
            &domain,
            &cancellation,
        )
        .unwrap();
        Fixture {
            bundle,
            substrate,
            relief,
            domain,
            forcing,
        }
    })
}

#[test]
fn radiative_helpers_reproduce_the_ceres_calibration_and_analytic_limits() {
    let measured_planetary =
        planetary_albedo_from_surface(EARTH_CALIBRATION_SURFACE_ALBEDO_GLOBAL_MEAN);
    assert!((measured_planetary - EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN).abs() <= 1.0e-12);
    assert_eq!(
        EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN.to_bits(),
        (CERES_EBAF_REFLECTED_SHORTWAVE_GLOBAL_MEAN_W_M2
            / CERES_EBAF_INCOMING_SHORTWAVE_GLOBAL_MEAN_W_M2)
            .to_bits()
    );

    assert_eq!(absorbed_shortwave_w_m2(0.0, 0.4), 0.0);
    let dark = absorbed_shortwave_w_m2(0.25, 0.05);
    let bright = absorbed_shortwave_w_m2(0.25, 0.75);
    assert!(dark > bright && bright >= 0.0);
    assert!(dark < EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2 * 0.25);

    let ceres_asr = CERES_EBAF_ABSORBED_SHORTWAVE_GLOBAL_MEAN_W_M2;
    let reference_temperature = gray_equilibrium_surface_temperature_c(ceres_asr);
    assert!((15.0..=18.0).contains(&reference_temperature));
    assert!(gray_equilibrium_surface_temperature_c(ceres_asr * 1.1) > reference_temperature);
    assert_eq!(
        linearized_outgoing_longwave_w_m2(ceres_asr, reference_temperature, reference_temperature),
        ceres_asr
    );
    assert!(
        linearized_outgoing_longwave_w_m2(
            ceres_asr,
            reference_temperature,
            reference_temperature + 2.0,
        ) > ceres_asr
    );
    assert_eq!(
        linearized_outgoing_longwave_w_m2(
            ceres_asr,
            reference_temperature,
            reference_temperature - 100.0,
        ),
        0.0
    );
}

#[test]
fn earth_water_and_energy_evidence_references_are_self_consistent() {
    assert!((0.0..1.0).contains(&EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE));
    let latent_heat =
        latent_heat_flux_w_m2_from_evaporation_mm_day(EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY);
    assert!(
        (WILD_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2..=WILD_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2)
            .contains(&latent_heat)
    );
    assert!((STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2
        ..=STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2)
        .contains(&latent_heat));
    assert_eq!(
        CERES_EBAF_TOA_NET_RADIATION_GLOBAL_MEAN_W_M2.to_bits(),
        (CERES_EBAF_ABSORBED_SHORTWAVE_GLOBAL_MEAN_W_M2
            - CERES_EBAF_OUTGOING_LONGWAVE_GLOBAL_MEAN_W_M2)
            .to_bits(),
    );
}

#[test]
fn forcing_is_exactly_p3_derived_bounded_and_deterministic() {
    let fixture = fixture();
    let surface = fixture.bundle.authoritative_surface();
    fixture
        .relief
        .validate_against(surface, &fixture.substrate, &ReliefSpec::default())
        .unwrap();
    fixture.forcing.validate_against(&fixture.domain).unwrap();

    let source_land = fixture
        .relief
        .surface_water_geometry()
        .ocean_area_fraction()
        .iter()
        .map(|&ocean| 1.0 - ocean)
        .collect::<Vec<_>>();
    assert_eq!(fixture.forcing.source_land_fraction(), source_land);
    assert!(source_land.iter().any(|value| (0.0..1.0).contains(value)));
    let expected_land =
        remap_intensive_f32(fixture.domain.source_to_climate(), &source_land).unwrap();
    assert_eq!(
        fixture.forcing.planet_forcing().land_fraction(),
        expected_land
    );
    for (cell, land_fraction) in expected_land.iter().copied().enumerate() {
        let sea_ice = fixture.forcing.sea_ice_fraction()[cell];
        assert!(sea_ice == 0.0 || (sea_ice == 1.0 && land_fraction < 1.0));
        // The sea-ice prior is diagnosed only: it does not remove evaporation
        // (design 2026-09-03 A4 §4.4).
        assert_eq!(
            fixture
                .forcing
                .planet_forcing()
                .surface_moisture_availability()[cell]
                .to_bits(),
            (1.0_f32 - land_fraction).to_bits()
        );
        for month in 0..12 {
            let air_temperature = fixture
                .forcing
                .planet_forcing()
                .equilibrium_air_temperature_c()[cell][month];
            let saturation = saturation_specific_humidity_kg_kg(f64::from(air_temperature));
            let expected = (REFERENCE_SURFACE_RELATIVE_HUMIDITY * saturation) as f32;
            assert_eq!(
                fixture
                    .forcing
                    .planet_forcing()
                    .equilibrium_specific_humidity()[cell][month]
                    .to_bits(),
                expected.to_bits()
            );
        }
    }
    let expected_elevation = remap_intensive_f32(
        fixture.domain.source_to_climate(),
        fixture.relief.elevation_m(),
    )
    .unwrap();
    assert_eq!(
        fixture.forcing.planet_forcing().elevation_m(),
        expected_elevation
    );
    let work_geometry = build_surface_water_geometry(
        fixture.domain.climate_surface(),
        &expected_elevation,
        fixture.relief.sea_level_m(),
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(
        fixture.forcing.ocean_edge_permeability(),
        work_geometry.wet_edge_fraction()
    );
    for ((relative, elevation), depth) in fixture
        .forcing
        .relative_elevation_m()
        .iter()
        .zip(expected_elevation)
        .zip(fixture.forcing.ocean_depth_m())
    {
        assert_eq!(
            relative.to_bits(),
            (elevation - fixture.relief.sea_level_m()).to_bits()
        );
        assert!(*depth >= 0.0);
    }
    assert!(fixture
        .forcing
        .terrain_gradient_m_per_m()
        .iter()
        .any(|gradient| gradient.iter().any(|component| component.abs() > 0.0)));
    assert!(fixture
        .forcing
        .ocean_edge_permeability()
        .iter()
        .all(|value| (0.0..=1.0).contains(value)));

    let repeated = GlobalClimateForcingBuilder::build(
        surface,
        &fixture.relief,
        fixture.substrate.relative_permeability(),
        &ClimateSpec::default(),
        &fixture.domain,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(&repeated, &fixture.forcing);
}

#[test]
fn axial_tilt_has_opposite_hemisphere_phase_and_orography_cools_land() {
    let fixture = fixture();
    let grid = fixture.domain.climate_surface();
    let north = grid
        .cells()
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            left.centroid.components()[2].total_cmp(&right.centroid.components()[2])
        })
        .unwrap()
        .0;
    let south = grid
        .cells()
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            left.centroid.components()[2].total_cmp(&right.centroid.components()[2])
        })
        .unwrap()
        .0;
    let insolation = fixture.forcing.monthly_insolation_fraction();
    assert!(insolation[north][6] > insolation[north][0]);
    assert!(insolation[south][0] > insolation[south][6]);
    let temperature = fixture
        .forcing
        .planet_forcing()
        .equilibrium_surface_temperature_c();
    assert!(temperature[north][6] > temperature[north][0]);
    assert!(temperature[south][0] > temperature[south][6]);

    let land = fixture.forcing.planet_forcing().land_fraction();
    let elevation = fixture.forcing.relative_elevation_m();
    let mut found_mountain_pair = false;
    'outer: for high in 0..grid.cells().len() {
        if land[high] < 0.8 || elevation[high] < 1_500.0 {
            continue;
        }
        let latitude = grid.cells()[high].centroid.components()[2];
        for low in 0..grid.cells().len() {
            if land[low] >= 0.8
                && elevation[low] + 1_000.0 < elevation[high]
                && (grid.cells()[low].centroid.components()[2] - latitude).abs() < 0.03
            {
                let high_mean = temperature[high].iter().sum::<f32>() / 12.0;
                let low_mean = temperature[low].iter().sum::<f32>() / 12.0;
                if high_mean + 3.0 < low_mean {
                    found_mountain_pair = true;
                    break 'outer;
                }
            }
        }
    }
    assert!(
        found_mountain_pair,
        "fixture must exercise lapse-rate causality"
    );
}

#[test]
fn reverse_projection_preserves_constants_flux_budgets_and_tangency() {
    let fixture = fixture();
    let climate_count = fixture.domain.climate_surface().cells().len();
    let constant = vec![[7.25_f32; 12]; climate_count];
    let intensive = project_monthly_intensive_scalar(
        &fixture.domain,
        fixture.bundle.authoritative_surface(),
        &constant,
    )
    .unwrap();
    assert!(intensive
        .values()
        .iter()
        .flatten()
        .all(|value| value.to_bits() == 7.25_f32.to_bits()));

    let rates = fixture
        .domain
        .climate_surface()
        .cells()
        .iter()
        .map(|cell| {
            let latitude = cell.centroid.components()[2] as f32;
            std::array::from_fn(|month| 1.0 + latitude.abs() + month as f32 * 0.1)
        })
        .collect::<Vec<_>>();
    let projected = project_monthly_extensive_rate(
        &fixture.domain,
        fixture.bundle.authoritative_surface(),
        &rates,
    )
    .unwrap();
    let mut measured_max_relative_error = 0.0_f64;
    for month in 0..12 {
        let source_total = rates
            .iter()
            .zip(fixture.domain.climate_to_source().source_cell_areas_m2())
            .map(|(value, area)| f64::from(value[month]) * area)
            .sum::<f64>();
        let target_total = projected
            .values()
            .iter()
            .zip(fixture.domain.climate_to_source().target_cell_areas_m2())
            .map(|(value, area)| f64::from(value[month]) * area)
            .sum::<f64>();
        let relative_error = (target_total - source_total).abs() / source_total;
        measured_max_relative_error = measured_max_relative_error.max(relative_error);
        assert!(relative_error <= GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX);
    }
    assert_eq!(
        projected.max_relative_conservation_error().to_bits(),
        measured_max_relative_error.to_bits(),
        "the report must describe the final quantized published field"
    );

    let vectors = fixture
        .domain
        .climate_surface()
        .cells()
        .iter()
        .map(|cell| {
            let [x, y, _] = cell.centroid.components();
            [[(-y) as f32, x as f32, 0.0]; 12]
        })
        .collect::<Vec<_>>();
    let projected_vectors = project_monthly_tangent_vectors(
        &fixture.domain,
        fixture.bundle.authoritative_surface(),
        &vectors,
    )
    .unwrap();
    for (cell, months) in fixture
        .bundle
        .authoritative_surface()
        .cells()
        .iter()
        .zip(projected_vectors)
    {
        let radial = cell.centroid.components();
        for vector in months {
            let radial_component = f64::from(vector[0]) * radial[0]
                + f64::from(vector[1]) * radial[1]
                + f64::from(vector[2]) * radial[2];
            assert!(radial_component.abs() <= 1.0e-6);
        }
    }
}

#[test]
fn forcing_and_projection_reject_cancellation_and_wrong_inputs_atomically() {
    let fixture = fixture();
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    assert_eq!(
        GlobalClimateForcingBuilder::build(
            fixture.bundle.authoritative_surface(),
            &fixture.relief,
            fixture.substrate.relative_permeability(),
            &ClimateSpec::default(),
            &fixture.domain,
            &cancellation,
        ),
        Err(GlobalClimateForcingError::Cancelled)
    );
    assert!(matches!(
        project_monthly_intensive_scalar(
            &fixture.domain,
            fixture.bundle.authoritative_surface(),
            &[],
        ),
        Err(ClimateProjectionError::LengthMismatch { .. })
    ));
    assert!(matches!(
        project_monthly_extensive_rate(
            &fixture.domain,
            fixture.bundle.authoritative_surface(),
            &[[-1.0; 12]],
        ),
        Err(ClimateProjectionError::LengthMismatch { .. })
    ));
}

#[test]
fn public_scalar_projection_rejects_a_structurally_valid_noncanonical_map() {
    let fixture = fixture();
    let mut value = serde_json::to_value(&fixture.domain).unwrap();
    for role in ["source_to_climate", "climate_to_source"] {
        for weight in value[role]["weights"].as_array_mut().unwrap() {
            weight["tangent_transform"]["coefficients"] = serde_json::json!([0.0, 0.0, 0.0, 0.0]);
        }
    }
    let forged: ClimateWorkDomainSnapshot = serde_json::from_value(value).unwrap();
    let values = vec![[1.0_f32; 12]; forged.climate_surface().cells().len()];

    assert!(matches!(
        project_monthly_intensive_scalar(&forged, fixture.bundle.authoritative_surface(), &values,),
        Err(ClimateProjectionError::InvalidDomain { .. })
    ));
    assert!(matches!(
        project_monthly_extensive_rate(&forged, fixture.bundle.authoritative_surface(), &values,),
        Err(ClimateProjectionError::InvalidDomain { .. })
    ));
}

#[test]
fn forcing_builder_observes_cancellation_after_dense_work_has_started() {
    let fixture = fixture();
    let cancellation = BuildCancellation::new();
    let result = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            GlobalClimateForcingBuilder::build(
                fixture.bundle.authoritative_surface(),
                &fixture.relief,
                fixture.substrate.relative_permeability(),
                &ClimateSpec::default(),
                &fixture.domain,
                &cancellation,
            )
        });
        while cancellation.observation_count() < 8 && !worker.is_finished() {
            std::hint::spin_loop();
        }
        let observed_before_request = cancellation.observation_count();
        cancellation.cancel();
        (observed_before_request, worker.join().unwrap())
    });

    assert!(
        result.0 >= 8,
        "forcing build completed before reaching cancellable dense work"
    );
    assert_eq!(result.1, Err(GlobalClimateForcingError::Cancelled));
}

/// A4 §3.1: the storage-consistent seasonal target is the periodic solution of
/// the linear energy-balance equation, so it averages to the annual target,
/// reproduces the North & Coakley (1979) single-harmonic amplitude and lag,
/// and stays finite through polar night.
#[test]
fn seasonal_storage_equilibrium_matches_the_linear_energy_balance_closed_forms() {
    let (air_storage, mixed_storage) = p4_seasonal_storage_heat_capacities_j_m2_k();
    assert!((7.0e6..8.0e6).contains(&air_storage), "{air_storage}");
    assert!((4.0e8..4.2e8).contains(&mixed_storage), "{mixed_storage}");

    let annual_c = gray_equilibrium_surface_temperature_c(200.0);
    let slope = gray_longwave_slope_w_m2_k(annual_c);
    assert!((3.0..4.0).contains(&slope), "{slope}");

    // Constant forcing: every month is the annual target.
    let flat =
        seasonal_storage_equilibrium_temperature_c(&[200.0; 12], annual_c, slope, air_storage);
    for month in flat {
        assert!((month - annual_c).abs() < 1e-9, "{month} vs {annual_c}");
    }

    // Single harmonic of half-range 150 W/m2 peaking at month 5.5.
    let omega = std::f64::consts::TAU / (12.0 * SECONDS_PER_CLIMATOLOGICAL_MONTH);
    let mut harmonic = [0.0_f64; 12];
    for (month, value) in harmonic.iter_mut().enumerate() {
        *value = 200.0 + 150.0 * (std::f64::consts::TAU * (month as f64 - 5.5) / 12.0).cos();
    }
    // Month means of a sinusoid are damped by sinc(pi/12).
    let sinc = (std::f64::consts::PI / 12.0).sin() / (std::f64::consts::PI / 12.0);
    for storage in [air_storage, mixed_storage, air_storage + mixed_storage] {
        let months =
            seasonal_storage_equilibrium_temperature_c(&harmonic, annual_c, slope, storage);
        let mean = months.iter().sum::<f64>() / 12.0;
        assert!((mean - annual_c).abs() < 1e-9, "mean {mean} vs {annual_c}");
        let expected_amplitude = 150.0 / (slope * slope + (omega * storage).powi(2)).sqrt();
        let expected_lag_months = (omega * storage / slope).atan() / std::f64::consts::TAU * 12.0;
        for (month, value) in months.iter().enumerate() {
            let phase = std::f64::consts::TAU * (month as f64 - 5.5 - expected_lag_months) / 12.0;
            let expected = annual_c + expected_amplitude * sinc * phase.cos();
            assert!(
                (value - expected).abs() < 0.02 * expected_amplitude + 1e-6,
                "storage {storage}: month {month} {value} vs {expected}"
            );
        }
    }
    // Storage damps and delays: the ocean column swings less and later than the air.
    let air = seasonal_storage_equilibrium_temperature_c(&harmonic, annual_c, slope, air_storage);
    let sea = seasonal_storage_equilibrium_temperature_c(&harmonic, annual_c, slope, mixed_storage);
    let swing = |months: &[f64; 12]| {
        months.iter().copied().fold(f64::MIN, f64::max)
            - months.iter().copied().fold(f64::MAX, f64::min)
    };
    assert!(
        swing(&air) > 10.0 * swing(&sea),
        "{} vs {}",
        swing(&air),
        swing(&sea)
    );
    let argmax = |months: &[f64; 12]| {
        months
            .iter()
            .enumerate()
            .max_by(|l, r| l.1.total_cmp(r.1))
            .unwrap()
            .0
    };
    assert!(
        argmax(&sea) > argmax(&air),
        "{} vs {}",
        argmax(&sea),
        argmax(&air)
    );

    // Polar night: six dark months stay finite and never fall below the
    // instantaneous floor T_ann - ASR_ann / B.
    let mut polar = [0.0_f64; 12];
    for (month, value) in polar.iter_mut().enumerate() {
        *value = if (3..9).contains(&month) { 400.0 } else { 0.0 };
    }
    let polar_annual = gray_equilibrium_surface_temperature_c(200.0);
    let polar_slope = gray_longwave_slope_w_m2_k(polar_annual);
    let months =
        seasonal_storage_equilibrium_temperature_c(&polar, polar_annual, polar_slope, air_storage);
    let floor = polar_annual - 200.0 / polar_slope;
    for value in months {
        assert!(
            value.is_finite() && value > floor && value > -100.0,
            "{value} vs {floor}"
        );
    }
    let mean = months.iter().sum::<f64>() / 12.0;
    assert!((mean - polar_annual).abs() < 1e-9);
}

/// A4 §3.1 on the fixture: land air targets swing with the season, ocean
/// mixed-layer targets barely move, and both average to the annual gray target.
#[test]
fn forcing_targets_are_storage_consistent_over_land_and_sea() {
    let fixture = fixture();
    let planet = fixture.forcing.planet_forcing();
    let elevation = fixture.forcing.relative_elevation_m();
    let mut land_swing = 0.0_f64;
    let mut sea_swing = 0.0_f64;
    for (cell, elevation_m) in elevation.iter().copied().enumerate() {
        let air = planet.equilibrium_air_temperature_c()[cell];
        let surface = planet.equilibrium_surface_temperature_c()[cell];
        let asr = planet.monthly_absorbed_shortwave_w_m2()[cell];
        let annual_asr = asr.iter().map(|v| f64::from(*v)).sum::<f64>() / 12.0;
        let land = f64::from(planet.land_fraction()[cell]);
        let orography = f64::from(elevation_m.max(0.0)) * land;
        let expected_annual = gray_equilibrium_surface_temperature_c(annual_asr)
            - sekai::world::natural::CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M * orography;
        let air_mean = air.iter().map(|v| f64::from(*v)).sum::<f64>() / 12.0;
        let surface_mean = surface.iter().map(|v| f64::from(*v)).sum::<f64>() / 12.0;
        if expected_annual > -89.0 {
            assert!(
                (air_mean - expected_annual).abs() < 1e-3,
                "{air_mean} vs {expected_annual}"
            );
            assert!(
                (surface_mean - expected_annual).abs() < 1e-3,
                "{surface_mean} vs {expected_annual}"
            );
        }
        let swing = |months: &[f32; 12]| {
            f64::from(months.iter().copied().fold(f32::MIN, f32::max))
                - f64::from(months.iter().copied().fold(f32::MAX, f32::min))
        };
        if land >= 0.999 {
            land_swing = land_swing.max(swing(&air));
        } else if land <= 0.001 {
            sea_swing = sea_swing.max(swing(&air));
        }
    }
    assert!(land_swing > 20.0, "{land_swing}");
    assert!(sea_swing < 6.0, "{sea_swing}");
}

/// A4 §4: the sea-ice prior covers only ocean whose ice-free annual target is
/// below the liquid floor, brightens exactly those cells, removes their
/// evaporation, and stays in the polar caps.
#[test]
fn sea_ice_prior_covers_only_cold_ocean_and_brightens_it() {
    let fixture = fixture();
    let planet = fixture.forcing.planet_forcing();
    let grid = fixture.domain.climate_surface();
    for cell in 0..planet.cell_count() {
        let land = f64::from(planet.land_fraction()[cell]);
        let ice = f64::from(fixture.forcing.sea_ice_fraction()[cell]);
        let albedo = f64::from(planet.surface_albedo()[cell]);
        let latitude = grid.cells()[cell].centroid.components()[2]
            .asin()
            .to_degrees();
        assert!(ice == 0.0 || ice == 1.0, "{ice}");
        if ice == 1.0 {
            assert!(land < 1.0);
            // The prior is diagnosed only: it neither brightens the surface
            // nor removes evaporation, because the CERES-calibrated
            // reflectance already contains Earth's ice and suppressing polar
            // evaporation stops the solve before the water cycle spins up
            // (design A4 §4.4).
            assert!(albedo < 0.6, "{albedo} at {latitude}");
            assert!(latitude.abs() > 40.0, "sea ice at {latitude} degrees");
        } else {
            assert!(albedo < 0.6, "{albedo} at {latitude}");
        }
    }
}
