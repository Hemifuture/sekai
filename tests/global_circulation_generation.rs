mod support;

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    ClimateWorkDomainBuilder, GlobalCirculationGenerationError, GlobalCirculationGenerator,
    GlobalClimateForcingBuilder, SELECTED_PRODUCTION_INTEGRATOR,
};
use sekai::world::natural::{
    ClimateCapabilityAvailability, ClimateCapabilityId, ClimateLayerRole, ClimateModelProfile,
    NaturalQualityProfile,
};

use support::global_circulation::global_circulation_fixture;

#[test]
fn c2_generation_publishes_every_semantic_field_and_exact_component_identity() {
    let fixture = global_circulation_fixture();
    let surface = fixture.bundle.authoritative_surface();
    let snapshot = GlobalCirculationGenerator::generate(
        surface,
        &fixture.domain,
        &fixture.forcing,
        ClimateModelProfile::C2LayeredV1,
        &BuildCancellation::new(),
    )
    .unwrap();
    snapshot.validate_against(surface).unwrap();
    assert_eq!(snapshot.integrator(), SELECTED_PRODUCTION_INTEGRATOR);
    assert_eq!(snapshot.profile(), ClimateModelProfile::C2LayeredV1);
    assert_eq!(snapshot.fields().cell_count(), surface.cells().len());
    assert!(snapshot.fields().upper_wind_m_s().is_some());
    assert!(snapshot.fields().vertical_wind_shear_m_s().is_some());
    assert!(snapshot
        .fields()
        .monthly_thermocline_temperature_c()
        .is_some());
    assert!(snapshot.fields().monthly_thermocline_depth_m().is_some());
    assert!(snapshot
        .fields()
        .monthly_deep_ocean_temperature_c()
        .is_some());
    assert_eq!(
        snapshot
            .capabilities()
            .availability(ClimateCapabilityId::VerticalStructureV1),
        ClimateCapabilityAvailability::Available
    );
    for unavailable in [
        ClimateCapabilityId::SeaIceV1,
        ClimateCapabilityId::LandSurfaceFeedbackV1,
        ClimateCapabilityId::EquatorialVariabilityV1,
        ClimateCapabilityId::TropicalCycloneClimatologyV1,
    ] {
        assert_eq!(
            snapshot.capabilities().availability(unavailable),
            ClimateCapabilityAvailability::Unavailable
        );
    }

    let lower = snapshot.fields().near_surface_wind_m_s().values();
    let upper = snapshot.fields().upper_wind_m_s().unwrap().values();
    let shear = snapshot
        .fields()
        .vertical_wind_shear_m_s()
        .unwrap()
        .values();
    for cell in 0..lower.len() {
        for month in 0..12 {
            for component in 0..3 {
                assert!(
                    (shear[cell][month][component]
                        - (upper[cell][month][component] - lower[cell][month][component]))
                        .abs()
                        <= 2.0e-4
                );
            }
        }
    }
}

#[test]
fn c1_generation_publishes_only_the_declared_single_layer_capabilities() {
    let fixture = global_circulation_fixture();
    let surface = fixture.bundle.authoritative_surface();
    let snapshot = GlobalCirculationGenerator::generate(
        surface,
        &fixture.domain,
        &fixture.forcing,
        ClimateModelProfile::C1SingleLayerV1,
        &BuildCancellation::new(),
    )
    .unwrap();
    snapshot.validate_against(surface).unwrap();
    assert_eq!(snapshot.profile(), ClimateModelProfile::C1SingleLayerV1);
    assert!(snapshot.fields().upper_wind_m_s().is_none());
    assert!(snapshot.fields().vertical_wind_shear_m_s().is_none());
    assert!(snapshot
        .fields()
        .monthly_thermocline_temperature_c()
        .is_none());
    assert!(snapshot.fields().monthly_thermocline_depth_m().is_none());
    assert_eq!(
        snapshot
            .capabilities()
            .availability(ClimateCapabilityId::VerticalStructureV1),
        ClimateCapabilityAvailability::Unavailable
    );
    assert!(snapshot
        .fields()
        .near_surface_wind_m_s()
        .values()
        .iter()
        .flatten()
        .any(|vector| vector.iter().any(|value| value.abs() > 0.05)));
}

#[test]
fn formation_is_convergent_budgeted_causal_and_deterministic() {
    let fixture = global_circulation_fixture();
    let surface = fixture.bundle.authoritative_surface();
    let run = || {
        GlobalCirculationGenerator::generate(
            surface,
            &fixture.domain,
            &fixture.forcing,
            ClimateModelProfile::C2LayeredV1,
            &BuildCancellation::new(),
        )
        .unwrap()
    };
    let first = run();
    let second = run();
    assert_eq!(first, second);
    println!(
        "solve={:?} budget={:?}",
        first.solve_report(),
        first.budget_report()
    );
    assert!(first.solve_report().formation_years() > 0);
    assert!(first.solve_report().macro_steps() >= 12);
    assert!(first.solve_report().fast_substeps() >= first.solve_report().macro_steps());
    assert!(
        first.solve_report().final_residual() <= 0.25,
        "formation residual {}",
        first.solve_report().final_residual()
    );
    first.budget_report().validate().unwrap();

    let fields = first.fields();
    let precipitation = fields.monthly_precipitation_mm_day().values();
    assert!(precipitation.iter().flatten().any(|value| *value > 0.01));
    let mixed = fields.monthly_sea_surface_temperature_c().values();
    let thermocline = fields.monthly_thermocline_temperature_c().unwrap().values();
    assert!(mixed.iter().zip(thermocline).any(|(mixed, thermocline)| {
        mixed
            .iter()
            .zip(thermocline)
            .any(|(mixed, thermocline)| mixed > thermocline)
    }));
    assert!(fields
        .monthly_thermocline_depth_m()
        .unwrap()
        .values()
        .iter()
        .flatten()
        .all(|value| *value > 0.0));
    assert!(first
        .fields()
        .near_surface_wind_m_s()
        .values()
        .iter()
        .flatten()
        .any(|vector| vector.iter().any(|value| value.abs() > 0.05)));
    assert!(first
        .fields()
        .surface_ocean_current_m_s()
        .values()
        .iter()
        .flatten()
        .any(|vector| vector.iter().any(|value| value.abs() > 0.001)));

    for role in [
        ClimateLayerRole::LowerAtmosphere,
        ClimateLayerRole::UpperAtmosphere,
        ClimateLayerRole::OceanMixedLayer,
        ClimateLayerRole::OceanThermocline,
    ] {
        assert!(first
            .layout()
            .layers()
            .iter()
            .any(|layer| layer.role() == role));
    }
}

#[test]
fn pre_cancelled_generation_publishes_no_partial_snapshot() {
    let fixture = global_circulation_fixture();
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    assert_eq!(
        GlobalCirculationGenerator::generate(
            fixture.bundle.authoritative_surface(),
            &fixture.domain,
            &fixture.forcing,
            ClimateModelProfile::C2LayeredV1,
            &cancellation,
        ),
        Err(GlobalCirculationGenerationError::Cancelled)
    );
}

#[test]
fn c2_cross_resolution_climatology_is_statistically_stable() {
    let fixture = global_circulation_fixture();
    let surface = fixture.bundle.authoritative_surface();
    let cancellation = BuildCancellation::new();
    let draft = GlobalCirculationGenerator::generate(
        surface,
        &fixture.domain,
        &fixture.forcing,
        ClimateModelProfile::C2LayeredV1,
        &cancellation,
    )
    .unwrap();
    let standard_domain =
        ClimateWorkDomainBuilder::build(surface, NaturalQualityProfile::Standard, &cancellation)
            .unwrap();
    let standard_forcing = GlobalClimateForcingBuilder::build(
        surface,
        &fixture.relief,
        &sekai::world::natural::ClimateSpec::default(),
        &standard_domain,
        &cancellation,
    )
    .unwrap();
    let standard = GlobalCirculationGenerator::generate(
        surface,
        &standard_domain,
        &standard_forcing,
        ClimateModelProfile::C2LayeredV1,
        &cancellation,
    )
    .unwrap();

    let draft_fields = draft.fields();
    let standard_fields = standard.fields();
    let comparisons = [
        (
            "air-temperature",
            scalar_mean(surface, draft_fields.monthly_air_temperature_c().values()),
            scalar_mean(
                surface,
                standard_fields.monthly_air_temperature_c().values(),
            ),
            2.0,
            false,
        ),
        (
            "specific-humidity",
            scalar_mean(surface, draft_fields.monthly_specific_humidity().values()),
            scalar_mean(
                surface,
                standard_fields.monthly_specific_humidity().values(),
            ),
            0.25,
            true,
        ),
        (
            "precipitation",
            scalar_mean(
                surface,
                draft_fields.monthly_precipitation_mm_day().values(),
            ),
            scalar_mean(
                surface,
                standard_fields.monthly_precipitation_mm_day().values(),
            ),
            0.30,
            true,
        ),
        (
            "near-surface-wind-rms",
            vector_rms(surface, draft_fields.near_surface_wind_m_s().values()),
            vector_rms(surface, standard_fields.near_surface_wind_m_s().values()),
            0.35,
            true,
        ),
        (
            "ocean-current-rms",
            vector_rms(surface, draft_fields.surface_ocean_current_m_s().values()),
            vector_rms(
                surface,
                standard_fields.surface_ocean_current_m_s().values(),
            ),
            0.45,
            true,
        ),
    ];
    for (name, draft_value, standard_value, tolerance, relative) in comparisons {
        let difference = if relative {
            (standard_value - draft_value).abs() / draft_value.abs().max(1.0e-9)
        } else {
            (standard_value - draft_value).abs()
        };
        println!(
            "{name}: draft={draft_value:.9} standard={standard_value:.9} difference={difference:.9}"
        );
        assert!(
            difference <= tolerance,
            "{name} cross-resolution difference {difference} exceeds {tolerance}"
        );
    }
}

fn scalar_mean(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    values: &[[f32; 12]],
) -> f64 {
    let total_area = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .sum::<f64>();
    surface
        .cells()
        .iter()
        .zip(values)
        .map(|(cell, months)| {
            cell.area.get() * months.iter().map(|value| f64::from(*value)).sum::<f64>() / 12.0
        })
        .sum::<f64>()
        / total_area
}

fn vector_rms(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    values: &[[[f32; 3]; 12]],
) -> f64 {
    let total_area = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .sum::<f64>();
    (surface
        .cells()
        .iter()
        .zip(values)
        .map(|(cell, months)| {
            cell.area.get()
                * months
                    .iter()
                    .map(|vector| {
                        vector
                            .iter()
                            .map(|value| f64::from(*value).powi(2))
                            .sum::<f64>()
                    })
                    .sum::<f64>()
                / 12.0
        })
        .sum::<f64>()
        / total_area)
        .sqrt()
}
