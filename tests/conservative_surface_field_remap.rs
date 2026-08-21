use sekai::generators::spatial::{
    remap_categories_u16, remap_extensive_f64, remap_extensive_f64_cancellable,
    remap_intensive_f32, remap_intensive_f32_cancellable, remap_intensive_f64,
    remap_tangent_components_f64, remap_tangent_components_f64_cancellable, ConservativeRemapError,
    ConservativeSurfaceMapBuilder, GeodesicVoronoiBuilder,
};
use sekai::world::spatial::{
    canonical_east_north_basis, ConservativeSurfaceMap, SurfaceGeometryKind, SurfaceOverlapWeight,
    SurfaceRef, TangentTransform, CONSERVATIVE_SURFACE_MAP_SCHEMA_V1, SPHERICAL_SURFACE_SCHEMA_V1,
};
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

const RADIUS_M: f64 = 6_371_000.0;

fn surface(target_cell_count: u32) -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(RADIUS_M).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn small_fixture() -> (
    sekai::world::spatial::SphericalSurfaceSnapshot,
    sekai::world::spatial::SphericalSurfaceSnapshot,
    ConservativeSurfaceMap,
) {
    let source = surface(42);
    let target = surface(162);
    let map = ConservativeSurfaceMapBuilder::build(&source, &target).unwrap();
    (source, target, map)
}

#[test]
fn constant_scalars_are_bit_exact_after_f64_and_f32_remapping() {
    let (source, target, map) = small_fixture();
    let constant_f64 = 0.123_456_789_f64;
    let remapped_f64 =
        remap_intensive_f64(&map, &vec![constant_f64; source.cells().len()]).unwrap();
    assert_eq!(remapped_f64.len(), target.cells().len());
    assert!(remapped_f64
        .iter()
        .all(|value| value.to_bits() == constant_f64.to_bits()));

    let constant_f32 = 0.123_456_79_f32;
    let remapped_f32 =
        remap_intensive_f32(&map, &vec![constant_f32; source.cells().len()]).unwrap();
    assert!(remapped_f32
        .iter()
        .all(|value| value.to_bits() == constant_f32.to_bits()));
}

#[test]
fn intensive_latitude_is_bounded_and_preserves_the_area_weighted_mean() {
    let (source, target, map) = small_fixture();
    let values = source
        .cells()
        .iter()
        .map(|cell| cell.centroid.components()[2])
        .collect::<Vec<_>>();
    let remapped = remap_intensive_f64(&map, &values).unwrap();
    let minimum = values.iter().copied().min_by(f64::total_cmp).unwrap();
    let maximum = values.iter().copied().max_by(f64::total_cmp).unwrap();
    assert!(remapped
        .iter()
        .all(|value| value.is_finite() && (minimum..=maximum).contains(value)));

    let source_integral = values
        .iter()
        .zip(source.cells())
        .map(|(value, cell)| value * cell.area.get())
        .sum::<f64>();
    let target_integral = remapped
        .iter()
        .zip(target.cells())
        .map(|(value, cell)| value * cell.area.get())
        .sum::<f64>();
    let scale = source
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .sum::<f64>();
    assert!((source_integral - target_integral).abs() / scale <= 1.0e-10);
}

#[test]
fn extensive_positive_and_signed_amounts_close_the_global_budget() {
    let (source, target, map) = small_fixture();
    let positive = (0..source.cells().len())
        .map(|index| index as f64 + 0.25)
        .collect::<Vec<_>>();
    let positive_result = remap_extensive_f64(&map, &positive).unwrap();
    assert_eq!(positive_result.values().len(), target.cells().len());
    assert!(positive_result.relative_error() <= 1.0e-6);
    assert!(
        (positive_result.source_total() - positive_result.target_total()).abs()
            <= positive_result.absolute_error() + f64::EPSILON
    );

    let signed = (0..source.cells().len())
        .map(|index| {
            let magnitude = index as f64 + 1.0;
            if index % 2 == 0 {
                magnitude
            } else {
                -magnitude
            }
        })
        .collect::<Vec<_>>();
    let signed_result = remap_extensive_f64(&map, &signed).unwrap();
    assert!(signed_result.relative_error() <= 1.0e-6);
    assert!(signed_result.values().iter().all(|value| value.is_finite()));
}

#[test]
fn downsampling_is_bounded_and_closes_intensive_and_extensive_budgets() {
    let source = surface(162);
    let target = surface(42);
    let map = ConservativeSurfaceMapBuilder::build(&source, &target).unwrap();
    let intensive = source
        .cells()
        .iter()
        .map(|cell| cell.centroid.components()[2])
        .collect::<Vec<_>>();
    let remapped_intensive = remap_intensive_f64(&map, &intensive).unwrap();
    let minimum = intensive.iter().copied().min_by(f64::total_cmp).unwrap();
    let maximum = intensive.iter().copied().max_by(f64::total_cmp).unwrap();
    assert!(remapped_intensive
        .iter()
        .all(|value| (minimum..=maximum).contains(value)));

    let source_integral = intensive
        .iter()
        .zip(source.cells())
        .map(|(value, cell)| value * cell.area.get())
        .sum::<f64>();
    let target_integral = remapped_intensive
        .iter()
        .zip(target.cells())
        .map(|(value, cell)| value * cell.area.get())
        .sum::<f64>();
    let area_scale = source
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .sum::<f64>();
    assert!((source_integral - target_integral).abs() / area_scale <= 1.0e-10);

    let extensive = source
        .cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| cell.area.get() * (0.5 + (index % 11) as f64))
        .collect::<Vec<_>>();
    let remapped_extensive = remap_extensive_f64(&map, &extensive).unwrap();
    assert!(remapped_extensive.relative_error() <= 1.0e-6);
    assert_eq!(remapped_extensive.values().len(), target.cells().len());
}

#[test]
fn solid_body_rotation_remains_tangent_and_directionally_aligned() {
    let (source, target, map) = small_fixture();
    let omega = [0.31, -0.27, 0.91];
    let source_components = source
        .cells()
        .iter()
        .map(|cell| local_solid_body_components(cell.centroid, omega))
        .collect::<Vec<_>>();
    let remapped = remap_tangent_components_f64(&map, &source_components).unwrap();

    let mut weighted_agreement = 0.0;
    let mut included_area = 0.0;
    for ((components, cell), area) in remapped
        .iter()
        .zip(target.cells())
        .zip(target.cells().iter().map(|cell| cell.area.get()))
    {
        let actual_global = global_from_local(cell.centroid, *components);
        let radial = cell.centroid.components();
        assert!(dot(actual_global, radial).abs() <= 1.0e-12);
        let expected = cross(omega, radial);
        let actual_norm = norm(actual_global);
        let expected_norm = norm(expected);
        if actual_norm > 1.0e-10 && expected_norm > 1.0e-10 {
            weighted_agreement +=
                area * dot(actual_global, expected) / (actual_norm * expected_norm);
            included_area += area;
        }
    }
    let agreement = weighted_agreement / included_area;
    assert!(
        agreement >= 0.999,
        "solid-body direction agreement {agreement}"
    );

    let identity = ConservativeSurfaceMapBuilder::build(&source, &source).unwrap();
    let repeated = remap_tangent_components_f64(&identity, &source_components).unwrap();
    for (found, expected) in repeated.iter().zip(source_components) {
        assert!((found[0] - expected[0]).abs() <= f64::EPSILON);
        assert!((found[1] - expected[1]).abs() <= f64::EPSILON);
    }
}

#[test]
fn category_majority_uses_a_stable_tie_break_and_reports_ambiguity() {
    let source_ref = SurfaceRef::new(
        SurfaceGeometryKind::SphericalV1,
        SPHERICAL_SURFACE_SCHEMA_V1,
        2,
        2,
        [1; 32],
    )
    .unwrap();
    let target_ref = SurfaceRef::new(
        SurfaceGeometryKind::SphericalV1,
        SPHERICAL_SURFACE_SCHEMA_V1,
        1,
        1,
        [2; 32],
    )
    .unwrap();
    let map = ConservativeSurfaceMap::new(
        CONSERVATIVE_SURFACE_MAP_SCHEMA_V1,
        source_ref,
        target_ref,
        vec![1.0, 1.0],
        vec![2.0],
        vec![0, 2],
        vec![
            SurfaceOverlapWeight::new(CellId::from_raw(0), 1.0, TangentTransform::identity())
                .unwrap(),
            SurfaceOverlapWeight::new(CellId::from_raw(1), 1.0, TangentTransform::identity())
                .unwrap(),
        ],
        0,
        0.0,
    )
    .unwrap();
    let result = remap_categories_u16(&map, &[5, 3]).unwrap();
    assert_eq!(result.values(), &[3]);
    assert_eq!(result.ambiguous_target_area_fraction(), 1.0);

    let identity_surface = surface(42);
    let identity =
        ConservativeSurfaceMapBuilder::build(&identity_surface, &identity_surface).unwrap();
    let categories = (0..identity_surface.cells().len())
        .map(|index| (index % 4) as u16)
        .collect::<Vec<_>>();
    let exact = remap_categories_u16(&identity, &categories).unwrap();
    assert_eq!(exact.values(), categories);
    assert_eq!(exact.ambiguous_target_area_fraction(), 0.0);
}

#[test]
fn field_remaps_reject_bad_lengths_and_non_finite_values_atomically() {
    let (source, _, map) = small_fixture();
    assert!(remap_intensive_f64(&map, &[1.0]).is_err());
    assert!(remap_extensive_f64(&map, &[1.0]).is_err());
    assert!(remap_tangent_components_f64(&map, &[[1.0, 0.0]]).is_err());
    assert!(remap_categories_u16(&map, &[1]).is_err());

    let mut non_finite = vec![1.0; source.cells().len()];
    non_finite[3] = f64::NAN;
    assert!(remap_intensive_f64(&map, &non_finite).is_err());
    assert!(remap_extensive_f64(&map, &non_finite).is_err());
    let mut vectors = vec![[1.0, 0.0]; source.cells().len()];
    vectors[3][1] = f64::INFINITY;
    assert!(remap_tangent_components_f64(&map, &vectors).is_err());
}

#[test]
fn climate_field_remap_kernels_observe_cancellation_inside_active_work() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (source, _, map) = small_fixture();
    let scalar_f32 = source
        .cells()
        .iter()
        .map(|cell| cell.centroid.components()[0] as f32)
        .collect::<Vec<_>>();
    let extensive = source
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .collect::<Vec<_>>();
    let tangent = source
        .cells()
        .iter()
        .map(|cell| local_solid_body_components(cell.centroid, [0.0, 0.0, 1.0]))
        .collect::<Vec<_>>();

    let observations = AtomicUsize::new(0);
    assert_eq!(
        remap_intensive_f32_cancellable(&map, &scalar_f32, &|| {
            observations.fetch_add(1, Ordering::Relaxed) >= 1
        })
        .unwrap_err(),
        ConservativeRemapError::Cancelled
    );

    let observations = AtomicUsize::new(0);
    assert_eq!(
        remap_extensive_f64_cancellable(&map, &extensive, &|| {
            observations.fetch_add(1, Ordering::Relaxed) >= 1
        })
        .unwrap_err(),
        ConservativeRemapError::Cancelled
    );

    let observations = AtomicUsize::new(0);
    assert_eq!(
        remap_tangent_components_f64_cancellable(&map, &tangent, &|| {
            observations.fetch_add(1, Ordering::Relaxed) >= 1
        })
        .unwrap_err(),
        ConservativeRemapError::Cancelled
    );
}

#[test]
fn draft_control_to_authoritative_field_remap_preserves_science_budgets() {
    let control = surface(4_842);
    let authoritative = surface(20_000);
    let map = ConservativeSurfaceMapBuilder::build(&control, &authoritative).unwrap();
    let scalar = vec![17.25_f32; control.cells().len()];
    let remapped_scalar = remap_intensive_f32(&map, &scalar).unwrap();
    assert!(remapped_scalar
        .iter()
        .all(|value| value.to_bits() == 17.25_f32.to_bits()));
    let extensive = control
        .cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| cell.area.get() * (1.0 + (index % 7) as f64))
        .collect::<Vec<_>>();
    let result = remap_extensive_f64(&map, &extensive).unwrap();
    assert!(result.relative_error() <= 1.0e-6);
}

fn local_solid_body_components(
    radial: sekai::world::spatial::UnitVector3,
    omega: [f64; 3],
) -> [f64; 2] {
    let global = cross(omega, radial.components());
    let (east, north) = canonical_east_north_basis(radial);
    [dot(global, east), dot(global, north)]
}

fn global_from_local(radial: sekai::world::spatial::UnitVector3, local: [f64; 2]) -> [f64; 3] {
    let (east, north) = canonical_east_north_basis(radial);
    [
        east[0] * local[0] + north[0] * local[1],
        east[1] * local[0] + north[1] * local[1],
        east[2] * local[0] + north[2] * local[1],
    ]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}
