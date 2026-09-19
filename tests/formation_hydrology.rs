use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use sekai::engine::BuildCancellation;
use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::generators::natural::{
    build_surface_water_geometry, global_circulation_model_fingerprint,
    FormationHydrologyGenerationError, FormationHydrologyGenerator,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, ProfileSurfaceBuilder};
use sekai::world::natural::{
    expected_global_circulation_dense_state_bytes, BasinOutletKind, BedrockKind, BedrockKindField,
    ClimateBudgetReport, ClimateCapabilitySet, ClimateCheckpoint, ClimateLayerLayout,
    ClimateModelProfile, ClimateQuantizationId, ClimateRemapReport, ClimateSolveReport, CrustKind,
    CrustKindField, FormationElevationComponents, FormationSedimentFields, FormationTerrainFields,
    GeologicSubstrateSnapshot, GlobalCirculationFields, GlobalCirculationSnapshot,
    HydroErosionSpec, MonthlyScalarField, MonthlyVector3Field, NaturalQualityProfile,
    ProductionIntegratorId, SedimentSourceKind, SedimentSourceKindField, SphericalMantleSnapshot,
    CLIMATOLOGICAL_YEAR_SECONDS, FORMATION_TERRAIN_FIELDS_SCHEMA_V4, GEOLOGIC_SUBSTRATE_SCHEMA_V1,
    GLOBAL_CIRCULATION_SCHEMA_V2, MANTLE_SNAPSHOT_SCHEMA_V2, SECONDS_PER_CLIMATOLOGICAL_MONTH,
};
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

fn surface(target_cell_count: u32) -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn zero_sediment(count: usize) -> FormationSedimentFields {
    FormationSedimentFields::new(
        vec![0.0; count],
        vec![[0.0; 5]; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
    )
    .unwrap()
}

fn terrain(surface: &SphericalSurfaceSnapshot, elevation_m: Vec<f32>) -> FormationTerrainFields {
    let count = elevation_m.len();
    let components = FormationElevationComponents::new(
        elevation_m.clone(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        elevation_m.clone(),
    )
    .unwrap();
    let water_geometry =
        build_surface_water_geometry(surface, &elevation_m, 0.0, &BuildCancellation::new())
            .unwrap();
    let water_volume_m3 = water_geometry.total_water_volume_m3();
    FormationTerrainFields::new(
        FORMATION_TERRAIN_FIELDS_SCHEMA_V4,
        components,
        water_geometry,
        water_volume_m3,
        zero_sediment(count),
    )
    .unwrap()
}

fn substrate(
    surface: &SphericalSurfaceSnapshot,
    relative_permeability: Vec<f32>,
) -> GeologicSubstrateSnapshot {
    let count = surface.cells().len();
    let surface_ref = SurfaceRef::for_spherical(surface);
    let mantle = SphericalMantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V2,
        surface_ref,
        Vec::new(),
        vec![65.0; count],
        vec![0.0; count],
    )
    .unwrap();
    GeologicSubstrateSnapshot::new(
        GEOLOGIC_SUBSTRATE_SCHEMA_V1,
        surface_ref,
        mantle,
        CrustKindField::from_kinds(vec![CrustKind::Continental; count]),
        vec![35.0; count],
        vec![-1.0; count],
        vec![2_800.0; count],
        BedrockKindField::from_kinds(vec![BedrockKind::ContinentalCrystalline; count]),
        vec![0.25; count],
        vec![0.30; count],
        relative_permeability,
        SedimentSourceKindField::from_kinds(vec![SedimentSourceKind::Felsic; count]),
    )
    .unwrap()
}

fn scalar(cell_count: usize, value: f32) -> MonthlyScalarField {
    MonthlyScalarField::from_values(vec![[value; 12]; cell_count]).unwrap()
}

fn vectors(cell_count: usize) -> MonthlyVector3Field {
    MonthlyVector3Field::from_values(vec![[[0.0; 3]; 12]; cell_count]).unwrap()
}

fn climate(
    surface: &SphericalSurfaceSnapshot,
    precipitation_mm_day: f32,
) -> GlobalCirculationSnapshot {
    let count = surface.cells().len();
    let fields = GlobalCirculationFields::new_c2(
        vectors(count),
        vectors(count),
        vectors(count),
        vectors(count),
        scalar(count, 12.0),
        scalar(count, 15.0),
        vec![0.1; count],
        scalar(count, 240.0),
        scalar(count, 240.0),
        scalar(count, 8.0),
        scalar(count, 900.0),
        scalar(count, 0.008),
        scalar(count, 0.0),
        scalar(count, precipitation_mm_day),
        scalar(count, 0.0),
        scalar(count, 0.0),
        scalar(count, 0.0),
        scalar(count, 0.0),
        scalar(count, 0.0),
        scalar(count, 4.0),
    )
    .unwrap();
    let checkpoint = ClimateCheckpoint::new(
        NaturalQualityProfile::Draft,
        ClimateModelProfile::C2LayeredV1,
        ProductionIntegratorId::SplitExplicitRk3V1,
        *CubedSphereGrid::new(
            NaturalQualityProfile::Draft.climate_face_resolution(),
            surface.radius().get(),
        )
        .unwrap()
        .fingerprint(),
        [21; 32],
        global_circulation_model_fingerprint(ClimateModelProfile::C2LayeredV1),
        [22; 32],
        ClimateQuantizationId::DeterministicF64V1,
        24,
        fields.fingerprint(),
    )
    .unwrap();
    GlobalCirculationSnapshot::new(
        GLOBAL_CIRCULATION_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1),
        ProductionIntegratorId::SplitExplicitRk3V1,
        ClimateCapabilitySet::for_profile(ClimateModelProfile::C2LayeredV1),
        checkpoint,
        ClimateSolveReport::new(
            2,
            24,
            144,
            0,
            1.0,
            0.1,
            0.5,
            expected_global_circulation_dense_state_bytes(
                NaturalQualityProfile::Draft,
                ClimateModelProfile::C2LayeredV1,
                count as u32,
            )
            .unwrap(),
        )
        .unwrap(),
        ClimateBudgetReport::new(0.0, 0.0, 0.0, 0.0, 0.0).unwrap(),
        ClimateRemapReport::new(0.0, 0.0, 0.0, 0.0, 0.0, 1, 1).unwrap(),
        fields,
    )
    .unwrap()
}

fn low_river_threshold_spec() -> HydroErosionSpec {
    HydroErosionSpec {
        river_discharge_threshold_deci_m3_s: 1,
        ..HydroErosionSpec::default()
    }
}

fn root_of(receivers: &[Option<CellId>], start: usize) -> usize {
    let mut cell = CellId::from_raw(start as u32);
    for _ in 0..=receivers.len() {
        match receivers[cell.raw() as usize] {
            Some(next) => cell = next,
            None => return cell.raw() as usize,
        }
    }
    panic!("receiver graph contains a cycle")
}

#[test]
fn p4_daily_rates_convert_to_monthly_runoff_and_close_discharge_and_area() {
    let surface = surface(42);
    let count = surface.cells().len();
    let mut elevation = vec![10.0; count];
    elevation[0] = -10.0;
    let terrain = terrain(&surface, elevation);
    let permeability = vec![0.25; count];
    let substrate = substrate(&surface, permeability);
    let climate = climate(&surface, 2.0);

    let first = FormationHydrologyGenerator::generate(
        &surface,
        &terrain,
        &substrate,
        &climate,
        &low_river_threshold_spec(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let second = FormationHydrologyGenerator::generate(
        &surface,
        &terrain,
        &substrate,
        &climate,
        &low_river_threshold_spec(),
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(first, second, "stable inputs must preserve IDs and bytes");
    assert!(first
        .basins()
        .iter()
        .all(|basin| basin.outlet_kind() == BasinOutletKind::Ocean));
    for index in 1..count {
        let root = root_of(first.flow_receiver(), index);
        assert_eq!(
            first.surface_water().get(root),
            Some(sekai::world::natural::SurfaceWaterKind::Ocean)
        );
    }

    let days_per_month = SECONDS_PER_CLIMATOLOGICAL_MONTH / 86_400.0;
    let expected_land_runoff_mm = 2.0 * days_per_month * (0.15 + 0.70 * 0.75);
    for index in 0..count {
        let expected = if index == 0 {
            0.0
        } else {
            expected_land_runoff_mm
        };
        for &found in &first.monthly_local_runoff_mm()[index] {
            assert!(
                (f64::from(found) - expected).abs() <= 2.0e-6,
                "cell {index}"
            );
        }
    }

    let mut root_area_m2 = BTreeMap::<usize, f64>::new();
    let mut root_annual_volume_m3 = BTreeMap::<usize, f64>::new();
    for index in 0..count {
        let root = root_of(first.flow_receiver(), index);
        let area_m2 = surface.cells()[index].area.get();
        *root_area_m2.entry(root).or_default() += area_m2;
        let local_annual_mm = first.monthly_local_runoff_mm()[index]
            .iter()
            .map(|&value| f64::from(value))
            .sum::<f64>();
        *root_annual_volume_m3.entry(root).or_default() += local_annual_mm * area_m2 / 1_000.0;
    }
    for (&root, &area_m2) in &root_area_m2 {
        let stored_area_m2 = f64::from(first.drainage_area_km2()[root]) * 1_000_000.0;
        assert!((stored_area_m2 - area_m2).abs() / area_m2 <= 2.0e-7);
        let expected_discharge = root_annual_volume_m3[&root] / CLIMATOLOGICAL_YEAR_SECONDS;
        let stored_discharge = f64::from(first.mean_annual_discharge_m3_s()[root]);
        let scale = expected_discharge.max(1.0);
        assert!((stored_discharge - expected_discharge).abs() / scale <= 2.0e-7);
    }
}

#[test]
fn one_meter_threshold_and_thousand_year_residence_control_lake_outflow() {
    let surface = surface(42);
    let count = surface.cells().len();
    let center = CellId::from_raw(0);
    let neighbors = surface
        .cell_edges(center)
        .unwrap()
        .iter()
        .map(|&edge| surface.opposite_cell(center, edge).unwrap())
        .collect::<Vec<_>>();
    let make_elevation = |center_height: f32| {
        let mut values = vec![-100.0; count];
        values[center.raw() as usize] = center_height;
        for neighbor in &neighbors {
            values[neighbor.raw() as usize] = 5.0;
        }
        values
    };
    let substrate = substrate(&surface, vec![0.25; count]);
    let spec = low_river_threshold_spec();

    let closed = FormationHydrologyGenerator::generate(
        &surface,
        &terrain(&surface, make_elevation(0.0)),
        &substrate,
        &climate(&surface, 1.0e-5),
        &spec,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(closed.lakes().len(), 1);
    assert_eq!(closed.lakes()[0].cells(), &[center]);
    assert_eq!(closed.lakes()[0].outlet_cell(), None);
    assert_eq!(closed.lakes()[0].downstream_cell(), None);
    assert!(
        closed
            .basins()
            .iter()
            .any(|basin| basin.outlet_cell() == center
                && basin.outlet_kind() == BasinOutletKind::Lake)
    );

    let spilled = FormationHydrologyGenerator::generate(
        &surface,
        &terrain(&surface, make_elevation(0.0)),
        &substrate,
        &climate(&surface, 1.0),
        &spec,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(spilled.lakes().len(), 1);
    assert_eq!(spilled.lakes()[0].outlet_cell(), Some(center));
    assert!(spilled.lakes()[0].downstream_cell().is_some());

    let insignificant = FormationHydrologyGenerator::generate(
        &surface,
        &terrain(&surface, make_elevation(4.5)),
        &substrate,
        &climate(&surface, 1.0),
        &HydroErosionSpec {
            minimum_lake_depth_cm: 1,
            ..spec
        },
        &BuildCancellation::new(),
    )
    .unwrap();
    assert!(insignificant.lakes().is_empty());

    for (index, receiver) in spilled.flow_receiver().iter().enumerate() {
        if let Some(receiver) = receiver {
            let cell = CellId::from_raw(index as u32);
            assert!(surface
                .cell_edges(cell)
                .unwrap()
                .iter()
                .any(|&edge| { surface.opposite_cell(cell, edge) == Some(*receiver) }));
            assert_ne!(root_of(spilled.flow_receiver(), index), usize::MAX);
        }
    }
    for segment in spilled.river_segments() {
        let from_is_internal_lake = spilled.lakes().iter().any(|lake| {
            lake.cells().contains(&segment.from()) && lake.outlet_cell() != Some(segment.from())
        });
        assert!(!from_is_internal_lake);
    }
}

#[test]
fn active_hydrology_work_observes_cancellation() {
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface().clone();
    let count = surface.cells().len();
    let terrain = terrain(&surface, vec![10.0; count]);
    let substrate = substrate(&surface, vec![0.25; count]);
    let climate = climate(&surface, 2.0);
    let signal = BuildCancellation::new();
    let worker_signal = signal.clone();
    let worker = std::thread::spawn(move || {
        FormationHydrologyGenerator::generate(
            &surface,
            &terrain,
            &substrate,
            &climate,
            &low_river_threshold_spec(),
            &worker_signal,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while signal.observation_count() < 32 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(
        signal.observation_count() >= 32,
        "worker never entered dense work"
    );
    signal.cancel();
    assert!(matches!(
        worker.join().unwrap(),
        Err(FormationHydrologyGenerationError::Cancelled)
    ));
}
