use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use sekai::generators::spatial::PlanarVoronoiBuilder;
use sekai::world::natural::{
    ElevationField, LandOceanField, LandOceanKind, ReliefSnapshot, SurfaceProcessSnapshot,
    SurfaceProcessValidationError, MAX_DEPOSITION_THICKNESS_M, MAX_EROSION_DEPTH_M,
    RELIEF_SCHEMA_V2, SURFACE_PROCESS_SCHEMA_V1,
};
use sekai::world::spatial::{SpatialSnapshot, Topology};
use sekai::world::{BoundaryCondition, CellId, Meters, PlanarSpaceSpec};

fn spatial_fixture(target_cell_count: u32) -> SpatialSnapshot {
    PlanarVoronoiBuilder::build(
        &PlanarSpaceSpec {
            width: Meters::new(1_000.0).unwrap(),
            height: Meters::new(500.0).unwrap(),
            target_cell_count,
            boundary: BoundaryCondition::Closed,
        },
        &mut ChaCha8Rng::seed_from_u64(17),
    )
    .unwrap()
}

fn relief_fixture(elevations: Vec<f32>) -> ReliefSnapshot {
    let cell_count = elevations.len() as u32;
    ReliefSnapshot::new(
        RELIEF_SCHEMA_V2,
        cell_count,
        0.0,
        ElevationField::from_values(elevations.clone()).unwrap(),
        ElevationField::from_values(vec![0.0; cell_count as usize]).unwrap(),
        ElevationField::from_values(vec![0.0; cell_count as usize]).unwrap(),
        ElevationField::from_values(vec![0.0; cell_count as usize]).unwrap(),
        ElevationField::from_values(elevations.clone()).unwrap(),
        LandOceanField::from_kinds(
            elevations
                .into_iter()
                .map(|elevation| LandOceanKind::classify(elevation, 0.0))
                .collect(),
        ),
    )
    .unwrap()
}

fn surface_fixture(
    spatial: &SpatialSnapshot,
    relief: &ReliefSnapshot,
    erosion_depth_m: Vec<f32>,
    deposition_thickness_m: Vec<f32>,
) -> SurfaceProcessSnapshot {
    let surface_elevation_m = relief
        .elevation_m()
        .values()
        .iter()
        .zip(&erosion_depth_m)
        .zip(&deposition_thickness_m)
        .map(|((&constructional, &erosion), &deposition)| constructional - erosion + deposition)
        .collect();
    let sediment_export_m3 = erosion_depth_m
        .iter()
        .zip(&deposition_thickness_m)
        .enumerate()
        .map(|(index, (&erosion, &deposition))| {
            let area = spatial
                .cell(CellId::from_raw(index as u32))
                .unwrap()
                .area
                .get();
            area * f64::from(erosion - deposition)
        })
        .sum();

    SurfaceProcessSnapshot::new(
        SURFACE_PROCESS_SCHEMA_V1,
        relief.cell_count(),
        erosion_depth_m,
        deposition_thickness_m,
        ElevationField::from_values(surface_elevation_m).unwrap(),
        vec![0.0; relief.cell_count() as usize],
        sediment_export_m3,
    )
    .unwrap()
}

fn valid_fixture() -> (SpatialSnapshot, ReliefSnapshot, SurfaceProcessSnapshot) {
    let spatial = spatial_fixture(16);
    let cell_count = spatial.cell_count();
    let relief = relief_fixture(vec![100.0; cell_count]);
    let surface = surface_fixture(
        &spatial,
        &relief,
        vec![2.0; cell_count],
        vec![0.5; cell_count],
    );
    (spatial, relief, surface)
}

#[test]
fn valid_surface_process_constructs_borrows_and_round_trips() {
    let (spatial, relief, snapshot) = valid_fixture();

    snapshot.validate().unwrap();
    snapshot.validate_against(&spatial, &relief).unwrap();
    assert_eq!(snapshot.schema_version(), SURFACE_PROCESS_SCHEMA_V1);
    assert_eq!(snapshot.cell_count(), relief.cell_count());
    assert_eq!(snapshot.erosion_depth_m()[0], 2.0);
    assert_eq!(snapshot.deposition_thickness_m()[0], 0.5);
    assert_eq!(snapshot.surface_elevation_m().values()[0], 98.5);
    assert_eq!(
        snapshot.sediment_throughput_m3().len(),
        relief.cell_count() as usize
    );
    assert!(snapshot.sediment_export_m3() > 0.0);

    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: SurfaceProcessSnapshot = serde_json::from_slice(&encoded).unwrap();
    decoded.validate_against(&spatial, &relief).unwrap();
    assert_eq!(decoded, snapshot);
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
}

#[test]
fn self_validation_rejects_schema_lengths_depth_bounds_and_surface_range() {
    let valid = valid_fixture().2;

    assert!(matches!(
        SurfaceProcessSnapshot::new(
            SURFACE_PROCESS_SCHEMA_V1 + 1,
            valid.cell_count(),
            valid.erosion_depth_m().to_vec(),
            valid.deposition_thickness_m().to_vec(),
            valid.surface_elevation_m().clone(),
            valid.sediment_throughput_m3().to_vec(),
            valid.sediment_export_m3(),
        ),
        Err(SurfaceProcessValidationError::UnsupportedSchema { .. })
    ));

    let mut short = valid.erosion_depth_m().to_vec();
    short.pop();
    assert!(matches!(
        SurfaceProcessSnapshot::new(
            SURFACE_PROCESS_SCHEMA_V1,
            valid.cell_count(),
            short,
            valid.deposition_thickness_m().to_vec(),
            valid.surface_elevation_m().clone(),
            valid.sediment_throughput_m3().to_vec(),
            valid.sediment_export_m3(),
        ),
        Err(SurfaceProcessValidationError::FieldLengthMismatch { .. })
    ));

    for invalid in [-1.0, f32::NAN, f32::INFINITY, MAX_EROSION_DEPTH_M + 1.0] {
        let mut erosion = valid.erosion_depth_m().to_vec();
        erosion[0] = invalid;
        assert!(matches!(
            SurfaceProcessSnapshot::new(
                SURFACE_PROCESS_SCHEMA_V1,
                valid.cell_count(),
                erosion,
                valid.deposition_thickness_m().to_vec(),
                valid.surface_elevation_m().clone(),
                valid.sediment_throughput_m3().to_vec(),
                valid.sediment_export_m3(),
            ),
            Err(SurfaceProcessValidationError::FieldValueOutOfRange { .. })
        ));
    }

    for invalid in [-1.0, f32::NEG_INFINITY, MAX_DEPOSITION_THICKNESS_M + 1.0] {
        let mut deposition = valid.deposition_thickness_m().to_vec();
        deposition[0] = invalid;
        assert!(matches!(
            SurfaceProcessSnapshot::new(
                SURFACE_PROCESS_SCHEMA_V1,
                valid.cell_count(),
                valid.erosion_depth_m().to_vec(),
                deposition,
                valid.surface_elevation_m().clone(),
                valid.sediment_throughput_m3().to_vec(),
                valid.sediment_export_m3(),
            ),
            Err(SurfaceProcessValidationError::FieldValueOutOfRange { .. })
        ));
    }

    assert!(matches!(
        SurfaceProcessSnapshot::new(
            SURFACE_PROCESS_SCHEMA_V1,
            valid.cell_count(),
            valid.erosion_depth_m().to_vec(),
            valid.deposition_thickness_m().to_vec(),
            ElevationField::from_values(vec![20_000.0; valid.cell_count() as usize]).unwrap(),
            valid.sediment_throughput_m3().to_vec(),
            valid.sediment_export_m3(),
        ),
        Err(SurfaceProcessValidationError::FieldValueOutOfRange { .. })
    ));
}

#[test]
fn sediment_volumes_must_be_finite_nonnegative_and_dense() {
    let valid = valid_fixture().2;

    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        let mut throughput = valid.sediment_throughput_m3().to_vec();
        throughput[0] = invalid;
        assert!(matches!(
            SurfaceProcessSnapshot::new(
                SURFACE_PROCESS_SCHEMA_V1,
                valid.cell_count(),
                valid.erosion_depth_m().to_vec(),
                valid.deposition_thickness_m().to_vec(),
                valid.surface_elevation_m().clone(),
                throughput,
                valid.sediment_export_m3(),
            ),
            Err(SurfaceProcessValidationError::InvalidSedimentVolume { .. })
        ));
    }

    for invalid in [-1.0, f64::NAN, f64::NEG_INFINITY] {
        assert!(matches!(
            SurfaceProcessSnapshot::new(
                SURFACE_PROCESS_SCHEMA_V1,
                valid.cell_count(),
                valid.erosion_depth_m().to_vec(),
                valid.deposition_thickness_m().to_vec(),
                valid.surface_elevation_m().clone(),
                valid.sediment_throughput_m3().to_vec(),
                invalid,
            ),
            Err(SurfaceProcessValidationError::InvalidSedimentVolume { .. })
        ));
    }

    let mut short = valid.sediment_throughput_m3().to_vec();
    short.pop();
    assert!(matches!(
        SurfaceProcessSnapshot::new(
            SURFACE_PROCESS_SCHEMA_V1,
            valid.cell_count(),
            valid.erosion_depth_m().to_vec(),
            valid.deposition_thickness_m().to_vec(),
            valid.surface_elevation_m().clone(),
            short,
            valid.sediment_export_m3(),
        ),
        Err(SurfaceProcessValidationError::FieldLengthMismatch { .. })
    ));
}

#[test]
fn cross_validation_enforces_surface_identity_and_sediment_conservation() {
    let (spatial, relief, valid) = valid_fixture();

    let mut wrong_surface = serde_json::to_value(&valid).unwrap();
    wrong_surface["surface_elevation_m"][0] = serde_json::json!(99.0);
    let wrong_surface: SurfaceProcessSnapshot = serde_json::from_value(wrong_surface).unwrap();
    assert!(matches!(
        wrong_surface.validate_against(&spatial, &relief),
        Err(SurfaceProcessValidationError::SurfaceIdentityMismatch { .. })
    ));

    let unbalanced = SurfaceProcessSnapshot::new(
        SURFACE_PROCESS_SCHEMA_V1,
        valid.cell_count(),
        valid.erosion_depth_m().to_vec(),
        valid.deposition_thickness_m().to_vec(),
        valid.surface_elevation_m().clone(),
        valid.sediment_throughput_m3().to_vec(),
        valid.sediment_export_m3() * 0.5,
    )
    .unwrap();
    assert!(matches!(
        unbalanced.validate_against(&spatial, &relief),
        Err(SurfaceProcessValidationError::SedimentMassMismatch { .. })
    ));
}

#[test]
fn ocean_cells_cannot_hide_fluvial_erosion_or_deposition() {
    let spatial = spatial_fixture(16);
    let count = spatial.cell_count();
    let mut elevation = vec![100.0; count];
    elevation[0] = -100.0;
    let relief = relief_fixture(elevation);
    let surface = surface_fixture(&spatial, &relief, vec![1.0; count], vec![0.0; count]);

    assert!(matches!(
        surface.validate_against(&spatial, &relief),
        Err(SurfaceProcessValidationError::OceanSurfaceProcess { .. })
    ));
}

#[test]
fn cross_validation_rejects_spatial_and_relief_cardinality_mismatch() {
    let (spatial, relief, surface) = valid_fixture();
    let other_spatial = spatial_fixture(25);
    assert!(matches!(
        surface.validate_against(&other_spatial, &relief),
        Err(SurfaceProcessValidationError::SpatialCellCountMismatch { .. })
    ));

    let short_relief = relief_fixture(vec![100.0; 15]);
    assert!(matches!(
        surface.validate_against(&spatial, &short_relief),
        Err(SurfaceProcessValidationError::ReliefCellCountMismatch { .. })
    ));
}

#[test]
fn invalid_json_cannot_bypass_constructor_validation() {
    let valid = valid_fixture().2;

    let mut negative = serde_json::to_value(&valid).unwrap();
    negative["erosion_depth_m"][0] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<SurfaceProcessSnapshot>(negative).is_err());

    let mut short = serde_json::to_value(&valid).unwrap();
    short["sediment_throughput_m3"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(serde_json::from_value::<SurfaceProcessSnapshot>(short).is_err());

    let mut non_finite = serde_json::to_string(&valid).unwrap();
    let value_start =
        non_finite.find("\"sediment_export_m3\":").unwrap() + "\"sediment_export_m3\":".len();
    let value_end = non_finite[value_start..].find('}').unwrap() + value_start;
    non_finite.replace_range(value_start..value_end, "1e999");
    assert!(serde_json::from_str::<SurfaceProcessSnapshot>(&non_finite).is_err());
}
