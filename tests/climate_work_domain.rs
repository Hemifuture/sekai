use sekai::engine::BuildCancellation;
use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::generators::natural::{ClimateWorkDomainBuildError, ClimateWorkDomainBuilder};
use sekai::generators::spatial::{remap_intensive_f32, GeodesicVoronoiBuilder};
use sekai::world::natural::{NaturalQualityProfile, CLIMATE_WORK_DOMAIN_SCHEMA_V1};
use sekai::world::spatial::{SurfaceGeometryKind, SurfaceRef, SPHERICAL_SURFACE_SCHEMA_V2};
use sekai::world::{Meters, SphericalSpaceSpec};

fn source_surface() -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 200,
    })
    .unwrap()
}

#[test]
fn cubed_sphere_converts_losslessly_to_a_valid_spherical_surface() {
    let grid = CubedSphereGrid::new(4, 6_371_000.0).unwrap();
    let first = grid.to_surface_snapshot().unwrap();
    let second = grid.to_surface_snapshot().unwrap();

    first.validate().unwrap();
    assert_eq!(first, second);
    assert_eq!(first.schema_version(), SPHERICAL_SURFACE_SCHEMA_V2);
    assert_eq!(
        SurfaceRef::for_spherical(&first).geometry_kind(),
        SurfaceGeometryKind::SphericalGeodesicV2
    );
    assert_eq!(first.cells().len(), grid.cell_count());
    assert_eq!(first.edges().len(), grid.edges().len());
    assert_eq!(first.vertices().len(), grid.vertex_count());
    let expected_total = grid.cells().iter().map(|cell| cell.area_m2()).sum::<f64>();
    assert!((first.total_cell_area().get() - expected_total).abs() / expected_total <= 2.0e-15);
    for (converted, original) in first.cells().iter().zip(grid.cells()) {
        assert_eq!(converted.id.raw(), original.id());
        assert_eq!(converted.site.components(), original.center_unit());
        assert_eq!(converted.area.get().to_bits(), original.area_m2().to_bits());
        assert_eq!(converted.boundary_vertices.len(), 4);
        assert_eq!(converted.boundary_edges.len(), 4);
    }
}

#[test]
fn work_domain_builds_exact_forward_and_reverse_conservative_maps() {
    let source = source_surface();
    let domain = ClimateWorkDomainBuilder::build(
        &source,
        NaturalQualityProfile::Draft,
        &BuildCancellation::new(),
    )
    .unwrap();

    domain.validate_against(&source).unwrap();
    assert_eq!(domain.schema_version(), CLIMATE_WORK_DOMAIN_SCHEMA_V1);
    assert_eq!(domain.profile(), NaturalQualityProfile::Draft);
    assert_eq!(domain.face_resolution(), 24);
    assert_eq!(domain.source_ref(), SurfaceRef::for_spherical(&source));
    assert_eq!(
        domain.source_to_climate().source_ref(),
        SurfaceRef::for_spherical(&source)
    );
    assert_eq!(
        domain.source_to_climate().target_ref(),
        SurfaceRef::for_spherical(domain.climate_surface())
    );
    assert_eq!(
        domain.climate_to_source().source_ref(),
        SurfaceRef::for_spherical(domain.climate_surface())
    );
    assert_eq!(
        domain.climate_to_source().target_ref(),
        SurfaceRef::for_spherical(&source)
    );

    let constant = vec![7.25_f32; source.cells().len()];
    let climate = remap_intensive_f32(domain.source_to_climate(), &constant).unwrap();
    let repeated = remap_intensive_f32(domain.climate_to_source(), &climate).unwrap();
    assert!(climate
        .iter()
        .all(|value| value.to_bits() == 7.25_f32.to_bits()));
    assert!(repeated
        .iter()
        .all(|value| value.to_bits() == 7.25_f32.to_bits()));
    assert!(
        domain
            .source_to_climate()
            .solve_stats()
            .max_source_margin_relative_error()
            <= 1.0e-12
    );
    assert!(
        domain
            .source_to_climate()
            .solve_stats()
            .max_target_margin_relative_error()
            <= 1.0e-12
    );
    assert!(
        domain
            .climate_to_source()
            .solve_stats()
            .max_source_margin_relative_error()
            <= 1.0e-12
    );
    assert!(
        domain
            .climate_to_source()
            .solve_stats()
            .max_target_margin_relative_error()
            <= 1.0e-12
    );
}

#[test]
fn work_domain_profile_resolution_and_serialization_are_strict_and_deterministic() {
    let source = source_surface();
    for profile in [
        NaturalQualityProfile::Draft,
        NaturalQualityProfile::Standard,
        NaturalQualityProfile::High,
    ] {
        let domain =
            ClimateWorkDomainBuilder::build(&source, profile, &BuildCancellation::new()).unwrap();
        assert_eq!(domain.face_resolution(), profile.climate_face_resolution());
        let bytes = serde_json::to_vec(&domain).unwrap();
        let decoded: sekai::world::natural::ClimateWorkDomainSnapshot =
            serde_json::from_slice(&bytes).unwrap();
        decoded.validate_against(&source).unwrap();
        assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());

        let mut value = serde_json::to_value(&domain).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unknown".to_owned(), serde_json::json!(true));
        assert!(
            serde_json::from_value::<sekai::world::natural::ClimateWorkDomainSnapshot>(value)
                .is_err()
        );
    }
}

#[test]
fn work_domain_snapshot_rejects_an_arbitrary_nonzero_grid_fingerprint() {
    let source = source_surface();
    let domain = ClimateWorkDomainBuilder::build(
        &source,
        NaturalQualityProfile::Draft,
        &BuildCancellation::new(),
    )
    .unwrap();
    let mut value = serde_json::to_value(domain).unwrap();
    let first = value["climate_grid_fingerprint"][0].as_u64().unwrap();
    value["climate_grid_fingerprint"][0] = serde_json::json!((first + 1) % 256);

    assert!(
        serde_json::from_value::<sekai::world::natural::ClimateWorkDomainSnapshot>(value).is_err(),
        "a self-consistent work-domain snapshot must bind the exact reconstructable grid"
    );
}

#[test]
fn public_work_domain_constructor_rejects_an_arbitrary_grid_fingerprint() {
    let source = source_surface();
    let domain = ClimateWorkDomainBuilder::build(
        &source,
        NaturalQualityProfile::Draft,
        &BuildCancellation::new(),
    )
    .unwrap();
    let mut wrong_fingerprint = *domain.climate_grid_fingerprint();
    wrong_fingerprint[0] = wrong_fingerprint[0].wrapping_add(1);

    assert!(sekai::world::natural::ClimateWorkDomainSnapshot::new(
        domain.schema_version(),
        domain.profile(),
        domain.face_resolution(),
        domain.source_ref(),
        wrong_fingerprint,
        domain.climate_surface().clone(),
        domain.source_to_climate().clone(),
        domain.climate_to_source().clone(),
    )
    .is_err());
}

#[test]
fn work_domain_snapshot_rejects_balanced_maps_scaled_off_their_surfaces() {
    let source = source_surface();
    let domain = ClimateWorkDomainBuilder::build(
        &source,
        NaturalQualityProfile::Draft,
        &BuildCancellation::new(),
    )
    .unwrap();
    let mut value = serde_json::to_value(domain).unwrap();
    for role in ["source_to_climate", "climate_to_source"] {
        let map = value[role].as_object_mut().unwrap();
        for field in ["source_cell_areas_m2", "target_cell_areas_m2"] {
            for area in map[field].as_array_mut().unwrap() {
                *area = serde_json::json!(area.as_f64().unwrap() * 0.5);
            }
        }
        for weight in map["weights"].as_array_mut().unwrap() {
            let area = weight["area_m2"].as_f64().unwrap();
            weight["area_m2"] = serde_json::json!(area * 0.5);
        }
    }

    assert!(
        serde_json::from_value::<sekai::world::natural::ClimateWorkDomainSnapshot>(value).is_err(),
        "balanced sparse margins must still be bound to the endpoint surface areas"
    );
}

#[test]
fn contextual_domain_validation_rejects_forged_tangent_transforms() {
    let source = source_surface();
    let domain = ClimateWorkDomainBuilder::build(
        &source,
        NaturalQualityProfile::Draft,
        &BuildCancellation::new(),
    )
    .unwrap();
    let mut value = serde_json::to_value(&domain).unwrap();
    for role in ["source_to_climate", "climate_to_source"] {
        for weight in value[role]["weights"].as_array_mut().unwrap() {
            weight["tangent_transform"]["coefficients"] = serde_json::json!([0.0, 0.0, 0.0, 0.0]);
        }
    }

    let forged: sekai::world::natural::ClimateWorkDomainSnapshot =
        serde_json::from_value(value).unwrap();
    assert!(
        forged.validate_against(&source).is_err(),
        "self-consistent sparse margins must not substitute for endpoint geometry"
    );
}

#[test]
fn work_domain_honors_pre_cancel_and_never_returns_a_partial_snapshot() {
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    assert_eq!(
        ClimateWorkDomainBuilder::build(
            &source_surface(),
            NaturalQualityProfile::Draft,
            &cancellation,
        ),
        Err(ClimateWorkDomainBuildError::Cancelled)
    );
}

#[test]
fn work_domain_honors_cancellation_after_grid_work_has_started() {
    let source = source_surface();
    let cancellation = BuildCancellation::new();
    let result = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            ClimateWorkDomainBuilder::build(&source, NaturalQualityProfile::Draft, &cancellation)
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
        "domain build ended before active cancellation"
    );
    assert_eq!(result.1, Err(ClimateWorkDomainBuildError::Cancelled));
}
