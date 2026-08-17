use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    project_monthly_extensive_rate, project_monthly_intensive_scalar,
    project_monthly_tangent_vectors, ClimateProjectionError, ClimateWorkDomainBuilder,
    EvolvedTectonicGenerator, GeologicSubstrateGenerator, GlobalClimateForcing,
    GlobalClimateForcingBuilder, GlobalClimateForcingError, PrimaryReliefGenerator,
};
use sekai::generators::spatial::{
    remap_intensive_f32, ProfileSurfaceBuilder, ProfileSurfaceBundle,
};
use sekai::world::natural::{
    ClimateSpec, ClimateWorkDomainSnapshot, GeologicSpec, GeologicSubstrateSnapshot, LandOceanKind,
    NaturalQualityProfile, PrimaryReliefSnapshot, ReliefSpec, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};

struct Fixture {
    bundle: ProfileSurfaceBundle,
    substrate: GeologicSubstrateSnapshot,
    relief: PrimaryReliefSnapshot,
    domain: ClimateWorkDomainSnapshot,
    forcing: GlobalClimateForcing,
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
        .land_ocean()
        .raw_values()
        .iter()
        .map(|&kind| f32::from(kind == LandOceanKind::Land.raw()))
        .collect::<Vec<_>>();
    let expected_land =
        remap_intensive_f32(fixture.domain.source_to_climate(), &source_land).unwrap();
    assert_eq!(
        fixture.forcing.planet_forcing().land_fraction(),
        expected_land
    );
    let expected_elevation = remap_intensive_f32(
        fixture.domain.source_to_climate(),
        fixture.relief.elevation_m(),
    )
    .unwrap();
    assert_eq!(
        fixture.forcing.planet_forcing().elevation_m(),
        expected_elevation
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
                assert!(high_mean + 3.0 < low_mean);
                found_mountain_pair = true;
                break 'outer;
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
    let intensive = project_monthly_intensive_scalar(&fixture.domain, &constant).unwrap();
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
    let projected = project_monthly_extensive_rate(&fixture.domain, &rates).unwrap();
    assert!(projected.max_relative_conservation_error() <= 1.0e-12);
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
        assert!((target_total - source_total).abs() / source_total <= 2.0e-7);
    }

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
            &ClimateSpec::default(),
            &fixture.domain,
            &cancellation,
        ),
        Err(GlobalClimateForcingError::Cancelled)
    );
    assert!(matches!(
        project_monthly_intensive_scalar(&fixture.domain, &[]),
        Err(ClimateProjectionError::LengthMismatch { .. })
    ));
    assert!(matches!(
        project_monthly_extensive_rate(&fixture.domain, &[[-1.0; 12]]),
        Err(ClimateProjectionError::LengthMismatch { .. })
    ));
}
