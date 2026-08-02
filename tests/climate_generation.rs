use std::sync::{Arc, OnceLock};

use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, ClimateGenerator, ClimateSpecArtifact,
    GeologicSpecArtifact, HydroErosionSpecArtifact, ReliefArtifact, RulePackSetArtifact,
    TectonicSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    ClimateSpec, ElevationField, GeologicSpec, HydroErosionSpec, LandOceanField,
    PreliminaryClimateSnapshot, ReliefSnapshot, TectonicSpec, RELIEF_SCHEMA_V2,
};
use sekai::world::spatial::Topology;
use sekai::world::{BoundaryCondition, CellId, Meters, PlanarSpaceSpec, RootSeed};

fn natural_artifacts() -> &'static (Arc<SpatialArtifact>, Arc<ReliefArtifact>) {
    static ARTIFACTS: OnceLock<(Arc<SpatialArtifact>, Arc<ReliefArtifact>)> = OnceLock::new();
    ARTIFACTS.get_or_init(|| {
        let mut external = ExternalArtifacts::new();
        external
            .insert(PlanarSpaceArtifact::new(PlanarSpaceSpec {
                width: Meters::new(1_600_000.0).unwrap(),
                height: Meters::new(1_000_000.0).unwrap(),
                target_cell_count: 512,
                boundary: BoundaryCondition::Closed,
            }))
            .unwrap();
        external
            .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
            .unwrap();
        external
            .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
            .unwrap();
        external
            .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
            .unwrap();
        external
            .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
            .unwrap();
        external
            .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
            .unwrap();
        external
            .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
            .unwrap();

        let outcome = BuildEngine::new(natural_foundation_graph().unwrap())
            .build(
                RootSeed::new(0xC1_1A_7E),
                external,
                &mut MemoryStageCache::new(),
            )
            .unwrap();
        (
            outcome.artifacts.get::<SpatialArtifact>().unwrap(),
            outcome.artifacts.get::<ReliefArtifact>().unwrap(),
        )
    })
}

fn generated(spec: &ClimateSpec) -> PreliminaryClimateSnapshot {
    let (spatial, relief) = natural_artifacts();
    ClimateGenerator::generate(spatial.snapshot(), relief.snapshot(), spec).unwrap()
}

fn synthetic_relief(
    spatial: &sekai::world::spatial::SpatialSnapshot,
    elevation: impl Fn(f32, f32) -> f32,
) -> ReliefSnapshot {
    let bounds = spatial.bounds();
    let min_x = bounds.min().x().get() as f32;
    let min_y = bounds.min().y().get() as f32;
    let width = bounds.width().get() as f32;
    let height = bounds.height().get() as f32;
    let values = (0..spatial.cell_count())
        .map(|index| {
            let site = spatial.cell(CellId::from_raw(index as u32)).unwrap().site;
            let x = ((site.x().get() as f32 - min_x) / width).clamp(0.0, 1.0);
            let y = ((site.y().get() as f32 - min_y) / height).clamp(0.0, 1.0);
            elevation(x, y)
        })
        .collect::<Vec<_>>();
    let cell_count = values.len();
    let zero = || ElevationField::from_values(vec![0.0; cell_count]).unwrap();
    let base = ElevationField::from_values(values.clone()).unwrap();
    let final_elevation = ElevationField::from_values(values).unwrap();
    let land_ocean = LandOceanField::classify(&final_elevation, 0.0);
    ReliefSnapshot::new(
        RELIEF_SCHEMA_V2,
        spatial.cell_count() as u32,
        0.0,
        base,
        zero(),
        zero(),
        zero(),
        final_elevation,
        land_ocean,
    )
    .unwrap()
}

fn mean(values: impl Iterator<Item = f32>) -> f32 {
    let (sum, count) = values.fold((0.0, 0_u32), |(sum, count), value| (sum + value, count + 1));
    assert!(count > 0);
    sum / count as f32
}

#[test]
fn generation_is_deterministic_dense_and_valid() {
    let (spatial, relief) = natural_artifacts();
    let first = generated(&ClimateSpec::default());
    let second = generated(&ClimateSpec::default());

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert_eq!(first.cell_count() as usize, spatial.snapshot().cell_count());
    first
        .validate_against(spatial.snapshot(), relief.snapshot())
        .unwrap();
    assert!(
        first
            .annual_precipitation_mm()
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min)
            < first
                .annual_precipitation_mm()
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max)
    );
}

#[test]
fn latitude_mapping_and_seasons_follow_planetary_geometry() {
    let spec = ClimateSpec::default();
    let climate = generated(&spec);
    let (spatial, _) = natural_artifacts();
    let south = (0..spatial.snapshot().cell_count())
        .min_by(|&left, &right| {
            climate.latitude_degrees()[left].total_cmp(&climate.latitude_degrees()[right])
        })
        .unwrap();
    let north = (0..spatial.snapshot().cell_count())
        .max_by(|&left, &right| {
            climate.latitude_degrees()[left].total_cmp(&climate.latitude_degrees()[right])
        })
        .unwrap();

    assert!(
        climate.latitude_degrees()[south] >= spec.south_latitude_degrees()
            && climate.latitude_degrees()[north] <= spec.north_latitude_degrees()
    );
    assert!(climate.latitude_degrees()[south] < -55.0);
    assert!(climate.latitude_degrees()[north] > 55.0);
    assert!(
        climate
            .air_temperature_c(CellId::from_raw(north as u32), 5)
            .unwrap()
            > climate
                .air_temperature_c(CellId::from_raw(north as u32), 11)
                .unwrap()
    );
    assert!(
        climate
            .air_temperature_c(CellId::from_raw(south as u32), 5)
            .unwrap()
            < climate
                .air_temperature_c(CellId::from_raw(south as u32), 11)
                .unwrap()
    );
}

#[test]
fn temperature_and_moisture_forcing_have_distinct_physical_effects() {
    let baseline = generated(&ClimateSpec::default());
    let (_, relief) = natural_artifacts();

    let low_latitude = mean(
        baseline
            .mean_annual_air_temperature_c()
            .iter()
            .enumerate()
            .filter(|(cell, _)| baseline.latitude_degrees()[*cell].abs() < 15.0)
            .map(|(cell, &temperature)| {
                temperature + relief.snapshot().elevation_m().values()[cell].max(0.0) * 0.0065
            }),
    );
    let high_latitude = mean(
        baseline
            .mean_annual_air_temperature_c()
            .iter()
            .enumerate()
            .filter(|(cell, _)| baseline.latitude_degrees()[*cell].abs() > 50.0)
            .map(|(cell, &temperature)| {
                temperature + relief.snapshot().elevation_m().values()[cell].max(0.0) * 0.0065
            }),
    );
    assert!(low_latitude > high_latitude + 12.0);

    let warm_spec = ClimateSpec {
        temperature_offset_deci_c: 100,
        ..ClimateSpec::default()
    };
    let warm = generated(&warm_spec);
    assert!(
        mean(warm.mean_annual_air_temperature_c().iter().copied())
            > mean(baseline.mean_annual_air_temperature_c().iter().copied()) + 8.0
    );

    let dry_spec = ClimateSpec {
        moisture_scale_permille: 500,
        ..ClimateSpec::default()
    };
    let wet_spec = ClimateSpec {
        moisture_scale_permille: 1_500,
        ..ClimateSpec::default()
    };
    let dry = generated(&dry_spec);
    let wet = generated(&wet_spec);
    assert!(
        mean(wet.annual_precipitation_mm().iter().copied())
            > mean(dry.annual_precipitation_mm().iter().copied()) * 1.5
    );
}

#[test]
fn maritime_influence_moderates_temperature_seasonality() {
    let (spatial, _) = natural_artifacts();
    let ocean_relief = synthetic_relief(spatial.snapshot(), |_, _| -1_000.0);
    let land_relief = synthetic_relief(spatial.snapshot(), |_, _| 100.0);
    let ocean =
        ClimateGenerator::generate(spatial.snapshot(), &ocean_relief, &ClimateSpec::default())
            .unwrap();
    let land =
        ClimateGenerator::generate(spatial.snapshot(), &land_relief, &ClimateSpec::default())
            .unwrap();

    assert!(ocean.maritime_influence().iter().all(|&value| value > 0.99));
    assert!(land.maritime_influence().iter().all(|&value| value < 0.01));
    assert!(
        mean(ocean.temperature_seasonality_c().iter().copied())
            < mean(land.temperature_seasonality_c().iter().copied()) * 0.7
    );
    ocean
        .validate_against(spatial.snapshot(), &ocean_relief)
        .unwrap();
    land.validate_against(spatial.snapshot(), &land_relief)
        .unwrap();
}

#[test]
fn westerly_moisture_transport_creates_a_ridge_rain_shadow() {
    let (spatial, _) = natural_artifacts();
    let relief = synthetic_relief(spatial.snapshot(), |x, _| {
        if x < 0.18 {
            -500.0
        } else if (0.46..=0.54).contains(&x) {
            2_800.0
        } else {
            100.0
        }
    });
    let climate =
        ClimateGenerator::generate(spatial.snapshot(), &relief, &ClimateSpec::default()).unwrap();
    let bounds = spatial.snapshot().bounds();
    let min_x = bounds.min().x().get() as f32;
    let min_y = bounds.min().y().get() as f32;
    let width = bounds.width().get() as f32;
    let height = bounds.height().get() as f32;
    let band = |x_min: f32, x_max: f32| {
        climate
            .annual_precipitation_mm()
            .iter()
            .enumerate()
            .filter_map(move |(index, &precipitation)| {
                let site = spatial
                    .snapshot()
                    .cell(CellId::from_raw(index as u32))
                    .unwrap()
                    .site;
                let x = (site.x().get() as f32 - min_x) / width;
                let y = (site.y().get() as f32 - min_y) / height;
                (x >= x_min && x < x_max && (0.72..0.88).contains(&y)).then_some(precipitation)
            })
    };
    let windward = mean(band(0.38, 0.48));
    let leeward = mean(band(0.56, 0.70));

    assert!(
        windward > leeward * 1.08,
        "windward={windward}, leeward={leeward}"
    );
}
