use super::{MetricAccumulator, MetricObservation, NaturalQualityReportBuilder, QualityBuildError};
use crate::generators::spatial::{
    remap_categories_u16, remap_extensive_f64, remap_intensive_f32, remap_tangent_components_f64,
};
use crate::world::natural::{NaturalQualityReport, QualityMetricId};
use crate::world::spatial::{
    canonical_east_north_basis, ConservativeSurfaceMap, SphericalSurfaceSnapshot, SurfaceRef,
};
use crate::world::CellId;

const CLOSED_SPHERE_AREA_RELATIVE_ERROR_MAX: f64 = 1.0e-10;
const SHARED_EDGE_FLUX_CANCELLATION_MAX: f64 = 1.0e-12;
const CONSTANT_SCALAR_MAX_ERROR: f64 = 0.0;
const EXTENSIVE_RELATIVE_ERROR_MAX: f64 = 1.0e-6;
const MARGIN_RELATIVE_ERROR_MAX: f64 = 1.0e-10;
const SOLID_BODY_DIRECTION_AGREEMENT_MIN: f64 = 0.999;
const TANGENT_RADIAL_RESIDUAL_MAX: f64 = 1.0e-12;
const CONSTANT_FIXTURE: f32 = 17.25;
const SOLID_BODY_OMEGA: [f64; 3] = [0.31, -0.27, 0.91];

pub(crate) fn evaluate_profile_surface_quality(
    authoritative: &SphericalSurfaceSnapshot,
    control: &SphericalSurfaceSnapshot,
    map: &ConservativeSurfaceMap,
) -> Result<NaturalQualityReport, QualityBuildError> {
    validate_input("authoritative surface", authoritative.validate())?;
    validate_input("tectonic-control surface", control.validate())?;
    validate_input("control-to-authoritative map", map.validate())?;
    let authoritative_ref = SurfaceRef::try_for_spherical(authoritative)
        .map_err(|error| invalid_input("authoritative surface identity", error))?;
    let control_ref = SurfaceRef::try_for_spherical(control)
        .map_err(|error| invalid_input("tectonic-control surface identity", error))?;
    if map.source_ref() != control_ref {
        return Err(QualityBuildError::SurfaceMismatch {
            input: "control-to-authoritative map source",
            found: map.source_ref(),
            expected: control_ref,
        });
    }
    if map.target_ref() != authoritative_ref {
        return Err(QualityBuildError::SurfaceMismatch {
            input: "control-to-authoritative map target",
            found: map.target_ref(),
            expected: authoritative_ref,
        });
    }

    let authoritative_cells = count("authoritative cells", authoritative.cells().len())?;
    let control_cells = count("tectonic-control cells", control.cells().len())?;
    let authoritative_edges = count("authoritative edges", authoritative.edges().len())?;
    let overlap_count = count("conservative overlaps", map.overlap_count())?;
    let mut builder = NaturalQualityReportBuilder::new(authoritative_ref);

    builder.record_at_most(
        metric_id("spatial", "closed-sphere-area-relative-error")?,
        closed_sphere_area_relative_error(authoritative),
        authoritative_cells,
        CLOSED_SPHERE_AREA_RELATIVE_ERROR_MAX,
    )?;
    builder.record_at_most(
        metric_id("spatial", "shared-edge-flux-cancellation-max")?,
        shared_edge_flux_cancellation_max(authoritative),
        authoritative_edges,
        SHARED_EDGE_FLUX_CANCELLATION_MAX,
    )?;

    let constant_source = vec![CONSTANT_FIXTURE; control.cells().len()];
    let constant_target = remap_intensive_f32(map, &constant_source)?;
    let constant_error = constant_target
        .iter()
        .map(|&value| f64::from((value - CONSTANT_FIXTURE).abs()))
        .max_by(f64::total_cmp)
        .unwrap_or(0.0);
    builder.record_at_most(
        metric_id("remap", "constant-scalar-max-error")?,
        constant_error,
        authoritative_cells,
        CONSTANT_SCALAR_MAX_ERROR,
    )?;

    let extensive_source = control
        .cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| cell.area.get() * (1.0 + (index % 7) as f64))
        .collect::<Vec<_>>();
    let extensive = remap_extensive_f64(map, &extensive_source)?;
    builder.record_at_most(
        metric_id("remap", "extensive-relative-error")?,
        extensive.relative_error(),
        control_cells,
        EXTENSIVE_RELATIVE_ERROR_MAX,
    )?;

    let stats = map.solve_stats();
    builder.record_at_most(
        metric_id("remap", "source-margin-max-relative-error")?,
        stats.max_source_margin_relative_error(),
        overlap_count,
        MARGIN_RELATIVE_ERROR_MAX,
    )?;
    builder.record_at_most(
        metric_id("remap", "target-margin-max-relative-error")?,
        stats.max_target_margin_relative_error(),
        overlap_count,
        MARGIN_RELATIVE_ERROR_MAX,
    )?;

    builder.record_observation_at_least(
        metric_id("remap", "solid-body-direction-agreement")?,
        solid_body_direction_agreement(control, authoritative, map)?,
        SOLID_BODY_DIRECTION_AGREEMENT_MIN,
    )?;

    let categories = control
        .cells()
        .iter()
        .map(|cell| u16::from(cell.centroid.components()[2] >= 0.0))
        .collect::<Vec<_>>();
    let categories = remap_categories_u16(map, &categories)?;
    builder.record_between(
        metric_id("remap", "category-ambiguity-area-fraction")?,
        categories.ambiguous_target_area_fraction(),
        authoritative_cells,
        0.0,
        1.0,
    )?;

    builder.finish()
}

fn closed_sphere_area_relative_error(surface: &SphericalSurfaceSnapshot) -> f64 {
    let radius = surface.radius().get();
    let analytic = 4.0 * std::f64::consts::PI * radius * radius;
    (surface.total_cell_area().get() - analytic).abs() / analytic
}

fn shared_edge_flux_cancellation_max(surface: &SphericalSurfaceSnapshot) -> f64 {
    surface
        .edges()
        .iter()
        .map(|edge| {
            let velocity = cross(SOLID_BODY_OMEGA, edge.midpoint.components());
            let normal = edge.normal_from_first.components();
            let first_flux = dot(velocity, normal) * edge.length.get();
            let second_flux = dot(velocity, scale(normal, -1.0)) * edge.length.get();
            let scale = first_flux.abs().max(second_flux.abs()).max(1.0);
            (first_flux + second_flux).abs() / scale
        })
        .max_by(f64::total_cmp)
        .unwrap_or(0.0)
}

fn solid_body_direction_agreement(
    control: &SphericalSurfaceSnapshot,
    authoritative: &SphericalSurfaceSnapshot,
    map: &ConservativeSurfaceMap,
) -> Result<MetricObservation, QualityBuildError> {
    let source = control
        .cells()
        .iter()
        .map(|cell| local_solid_body_components(cell.centroid))
        .collect::<Vec<_>>();
    let target = remap_tangent_components_f64(map, &source)?;
    let mut agreement = MetricAccumulator::new();
    for (index, (components, cell)) in target.iter().zip(authoritative.cells()).enumerate() {
        let actual = global_from_local(cell.centroid, *components);
        let radial = cell.centroid.components();
        let radial_residual = dot(actual, radial).abs();
        if radial_residual > TANGENT_RADIAL_RESIDUAL_MAX {
            return Err(QualityBuildError::TangentResidualExceeded {
                cell: CellId::from_raw(index as u32),
                found: radial_residual,
                max: TANGENT_RADIAL_RESIDUAL_MAX,
            });
        }
        let expected = cross(SOLID_BODY_OMEGA, radial);
        let actual_norm = norm(actual);
        let expected_norm = norm(expected);
        if actual_norm > 1.0e-10 && expected_norm > 1.0e-10 {
            let cosine = (dot(actual, expected) / (actual_norm * expected_norm)).clamp(-1.0, 1.0);
            agreement.push(cosine, cell.area.get())?;
        }
    }
    agreement.finish()
}

fn local_solid_body_components(radial: crate::world::spatial::UnitVector3) -> [f64; 2] {
    let global = cross(SOLID_BODY_OMEGA, radial.components());
    let (east, north) = canonical_east_north_basis(radial);
    [dot(global, east), dot(global, north)]
}

fn global_from_local(radial: crate::world::spatial::UnitVector3, local: [f64; 2]) -> [f64; 3] {
    let (east, north) = canonical_east_north_basis(radial);
    [
        east[0] * local[0] + north[0] * local[1],
        east[1] * local[0] + north[1] * local[1],
        east[2] * local[0] + north[2] * local[1],
    ]
}

fn metric_id(namespace: &str, name: &str) -> Result<QualityMetricId, QualityBuildError> {
    Ok(QualityMetricId::new(namespace, name, 1)?)
}

fn count(field: &'static str, value: usize) -> Result<u32, QualityBuildError> {
    u32::try_from(value).map_err(|_| QualityBuildError::CountOverflow {
        field,
        found: value,
    })
}

fn validate_input<E: std::fmt::Display>(
    input: &'static str,
    result: Result<(), E>,
) -> Result<(), QualityBuildError> {
    result.map_err(|error| invalid_input(input, error))
}

fn invalid_input(input: &'static str, error: impl std::fmt::Display) -> QualityBuildError {
    QualityBuildError::InvalidInput {
        input,
        reason: error.to_string(),
    }
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

fn scale(vector: [f64; 3], scale: f64) -> [f64; 3] {
    [vector[0] * scale, vector[1] * scale, vector[2] * scale]
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}
