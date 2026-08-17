//! Versioned scientific evidence for conservative V5 tectonics.

use std::collections::BTreeSet;
use std::f64::consts::PI;

use super::{MetricObservation, NaturalQualityReportBuilder, QualityBuildError};
use crate::world::natural::{
    BoundaryKind, CrustKind, EvolvedTectonicSnapshot, NaturalQualityReport, QualityMetricId,
    QualityMetricStatus, MAX_TECTONIC_AUTHORITY_RELATIVE_BUDGET_ERROR,
    MAX_TECTONIC_CONTROL_RELATIVE_BUDGET_ERROR,
};
use crate::world::spatial::{
    canonical_east_north_basis, project_tangent, SphericalSurfaceSnapshot,
};
use crate::world::{EdgeId, PlateId, SurfaceVertexId};

const METRIC_NAMESPACE: &str = "sekai.tectonics-v5";
const METRIC_VERSION: u16 = 1;
const MACRO_BRANCH_LENGTH_M: f64 = 750_000.0;
const EXPECTED_METRIC_NAMES: [&str; 13] = [
    "authority-material-relative-error",
    "collision-causality-fraction",
    "continental-area-fraction",
    "continental-area-retention",
    "control-material-relative-error",
    "lineage-closure-error",
    "maximum-plate-area-fraction",
    "non-finite-value-count",
    "ocean-age-depth-spearman",
    "regular-triple-junction-angle-fraction",
    "remap-category-ambiguity-fraction",
    "subduction-causality-fraction",
    "transform-to-convergent-uplift-ratio",
];
const CORPUS_SCOPED_METRICS: [&str; 6] = [
    "collision-causality-fraction",
    "continental-area-fraction",
    "ocean-age-depth-spearman",
    "regular-triple-junction-angle-fraction",
    "subduction-causality-fraction",
    "transform-to-convergent-uplift-ratio",
];

/// Evaluates every intrinsic P2 gate against one authoritative V5 snapshot.
///
/// Event-conditioned corpus metrics are explicitly unavailable when a single
/// world contains no matching event. A finite observation never silently
/// substitutes a default value for missing evidence.
pub fn evaluate_evolved_tectonic_quality(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &EvolvedTectonicSnapshot,
) -> Result<NaturalQualityReport, QualityBuildError> {
    surface
        .validate()
        .map_err(|error| invalid_input("surface", error.to_string()))?;
    snapshot
        .validate_against(surface)
        .map_err(|error| invalid_input("evolved-tectonics", error.to_string()))?;

    let mut builder = NaturalQualityReportBuilder::new(snapshot.surface_ref());
    let budget = *snapshot.material_budget();
    let initial = budget.initial_control();
    let final_control = budget.final_control();
    let final_authority = budget.final_authoritative();
    let total_authority = final_authority.continental().reference_area_m2()
        + final_authority.oceanic().reference_area_m2();
    let continental_fraction = checked_ratio(
        final_authority.continental().reference_area_m2(),
        total_authority,
        "continental-area-fraction",
    )?;
    let continental_retention = checked_ratio(
        final_control.continental().reference_area_m2(),
        initial.continental().reference_area_m2(),
        "continental-area-retention",
    )?;
    builder.record_between(
        metric_id("continental-area-fraction")?,
        continental_fraction,
        count(surface.cells().len(), "continental-area-fraction")?,
        0.30,
        0.45,
    )?;
    builder.record_between(
        metric_id("continental-area-retention")?,
        continental_retention,
        1,
        0.75,
        1.15,
    )?;

    let maximum_plate_fraction = maximum_plate_area_fraction(surface, snapshot)?;
    builder.record_at_most(
        metric_id("maximum-plate-area-fraction")?,
        maximum_plate_fraction,
        count(snapshot.compatibility().plates().len(), "plates")?,
        0.45,
    )?;

    let causality = boundary_causality(surface, snapshot)?;
    builder.record_observation_at_least(
        metric_id("subduction-causality-fraction")?,
        causality.subduction,
        0.80,
    )?;
    builder.record_observation_at_least(
        metric_id("collision-causality-fraction")?,
        causality.collision,
        0.80,
    )?;
    builder.record_observation_at_most(
        metric_id("transform-to-convergent-uplift-ratio")?,
        causality.transform_ratio,
        0.50,
    )?;

    builder.record_observation_at_least(
        metric_id("ocean-age-depth-spearman")?,
        ocean_age_depth_spearman(surface, snapshot)?,
        0.70,
    )?;
    builder.record_observation_at_most(
        metric_id("regular-triple-junction-angle-fraction")?,
        regular_triple_junction_fraction(surface, snapshot)?,
        0.35,
    )?;

    builder.record_at_most(
        metric_id("control-material-relative-error")?,
        budget.max_control_relative_error(),
        4,
        MAX_TECTONIC_CONTROL_RELATIVE_BUDGET_ERROR,
    )?;
    builder.record_at_most(
        metric_id("authority-material-relative-error")?,
        budget.max_authority_relative_error(),
        4,
        MAX_TECTONIC_AUTHORITY_RELATIVE_BUDGET_ERROR,
    )?;
    let lineage = *snapshot.lineage_budget();
    let created = u64::from(lineage.initial_lineages()) + u64::from(lineage.allocated_lineages());
    let accounted =
        u64::from(lineage.retired_lineages()) + u64::from(lineage.final_live_lineages());
    builder.record_at_most(
        metric_id("lineage-closure-error")?,
        created.abs_diff(accounted) as f64,
        1,
        0.0,
    )?;
    builder.record_unbounded(
        metric_id("remap-category-ambiguity-fraction")?,
        budget.category_ambiguity_area_fraction(),
        count(surface.cells().len(), "remap-category-ambiguity")?,
    )?;

    let (non_finite, inspected) = non_finite_count(snapshot);
    builder.record_at_most(
        metric_id("non-finite-value-count")?,
        non_finite as f64,
        count(inspected, "non-finite-value-count")?,
        0.0,
    )?;
    builder.finish()
}

/// Evaluates the six P2 statistical gates over a complete fixed-seed corpus.
///
/// Ratios are recombined from their contributing sample counts. Ocean
/// age/depth ranks and transform/convergent medians are recomputed from the
/// original authoritative cells rather than averaging per-world summaries.
pub fn evaluate_evolved_tectonic_corpus_quality(
    surface: &SphericalSurfaceSnapshot,
    snapshots: &[&EvolvedTectonicSnapshot],
) -> Result<NaturalQualityReport, QualityBuildError> {
    surface
        .validate()
        .map_err(|error| invalid_input("surface", error.to_string()))?;
    if snapshots.is_empty() {
        return Err(invalid_input(
            "evolved-tectonic-corpus",
            "the quality corpus is empty".to_owned(),
        ));
    }
    for snapshot in snapshots {
        snapshot
            .validate_against(surface)
            .map_err(|error| invalid_input("evolved-tectonic-corpus", error.to_string()))?;
    }

    let mut continental_fractions = Vec::with_capacity(snapshots.len());
    let mut subduction = FractionAggregate::default();
    let mut collision = FractionAggregate::default();
    let mut triple_regularity = FractionAggregate::default();
    let mut ocean_age_depth = Vec::new();
    let mut transform_uplift = Vec::new();
    let mut convergent_uplift = Vec::new();
    for snapshot in snapshots {
        let totals = snapshot.material_budget().final_authoritative();
        continental_fractions.push(checked_ratio(
            totals.continental().reference_area_m2(),
            totals.continental().reference_area_m2() + totals.oceanic().reference_area_m2(),
            "continental-area-fraction",
        )?);
        let causality = boundary_causality(surface, snapshot)?;
        subduction.push(&causality.subduction)?;
        collision.push(&causality.collision)?;
        triple_regularity.push(&regular_triple_junction_fraction(surface, snapshot)?)?;
        append_ocean_age_depth(surface, snapshot, &mut ocean_age_depth);
        append_boundary_uplift_samples(
            surface,
            snapshot,
            &mut transform_uplift,
            &mut convergent_uplift,
        )?;
    }

    continental_fractions.sort_by(f64::total_cmp);
    let mut builder = NaturalQualityReportBuilder::new(snapshots[0].surface_ref());
    builder.record_between(
        metric_id("continental-area-fraction")?,
        median_f64(&continental_fractions),
        count(snapshots.len(), "evolved-tectonic-corpus")?,
        0.30,
        0.45,
    )?;
    builder.record_observation_at_least(
        metric_id("subduction-causality-fraction")?,
        subduction.finish("no ocean-continent subduction edges in the corpus")?,
        0.80,
    )?;
    builder.record_observation_at_least(
        metric_id("collision-causality-fraction")?,
        collision.finish("no continental-collision edges in the corpus")?,
        0.80,
    )?;

    let ocean_observation = if ocean_age_depth.len() < 2 {
        MetricObservation::Unavailable {
            reason: "fewer than two oceanic cells in the corpus".to_owned(),
        }
    } else if let Some(value) = weighted_spearman(&ocean_age_depth) {
        MetricObservation::Available {
            value,
            sample_count: count(ocean_age_depth.len(), "corpus-ocean-age-depth")?,
        }
    } else {
        MetricObservation::Unavailable {
            reason: "corpus ocean age or depth ranks have zero weighted variance".to_owned(),
        }
    };
    builder.record_observation_at_least(
        metric_id("ocean-age-depth-spearman")?,
        ocean_observation,
        0.70,
    )?;
    builder.record_observation_at_most(
        metric_id("regular-triple-junction-angle-fraction")?,
        triple_regularity.finish("no three-lineage macro junctions in the corpus")?,
        0.35,
    )?;

    let transform_observation = if transform_uplift.is_empty() {
        MetricObservation::Unavailable {
            reason: "no transform edges in the corpus".to_owned(),
        }
    } else if convergent_uplift.is_empty() {
        MetricObservation::Unavailable {
            reason: "no convergent edges in the corpus".to_owned(),
        }
    } else {
        let transform = median(&mut transform_uplift);
        let convergent = median(&mut convergent_uplift);
        if convergent <= 0.0 {
            MetricObservation::Unavailable {
                reason: "corpus convergent median uplift is zero".to_owned(),
            }
        } else {
            MetricObservation::Available {
                value: checked_ratio(transform, convergent, "corpus-transform-uplift-ratio")?,
                sample_count: count(
                    transform_uplift.len() + convergent_uplift.len(),
                    "corpus-transform-uplift-ratio",
                )?,
            }
        }
    };
    builder.record_observation_at_most(
        metric_id("transform-to-convergent-uplift-ratio")?,
        transform_observation,
        0.50,
    )?;
    builder.finish()
}

#[derive(Default)]
struct FractionAggregate {
    numerator: f64,
    samples: u64,
}

impl FractionAggregate {
    fn push(&mut self, observation: &MetricObservation) -> Result<(), QualityBuildError> {
        if let MetricObservation::Available {
            value,
            sample_count,
        } = observation
        {
            let contribution = *value * f64::from(*sample_count);
            if !contribution.is_finite() {
                return Err(QualityBuildError::NonFiniteAccumulation);
            }
            self.numerator += contribution;
            self.samples = self
                .samples
                .checked_add(u64::from(*sample_count))
                .ok_or(QualityBuildError::SampleCountOverflow)?;
        }
        Ok(())
    }

    fn finish(self, empty_reason: &'static str) -> Result<MetricObservation, QualityBuildError> {
        if self.samples == 0 {
            return Ok(MetricObservation::Unavailable {
                reason: empty_reason.to_owned(),
            });
        }
        let sample_count =
            u32::try_from(self.samples).map_err(|_| QualityBuildError::SampleCountOverflow)?;
        Ok(MetricObservation::Available {
            value: self.numerator / self.samples as f64,
            sample_count,
        })
    }
}

/// Enforces the exact V5 metric inventory and every per-world hard gate.
///
/// Corpus-scoped metrics retain honest per-world `Fail` or `Unavailable`
/// observations. Their locked bounds are evaluated only after aggregating the
/// fixed 17-seed corpus.
pub(crate) fn validate_evolved_tectonic_quality_report(
    report: &NaturalQualityReport,
    expected_surface: crate::world::spatial::SurfaceRef,
) -> Result<(), String> {
    report.validate().map_err(|error| error.to_string())?;
    if report.surface_ref() != expected_surface {
        return Err("P2 quality report is not bound to the evolved tectonic authority".to_owned());
    }
    if report.metrics().len() != EXPECTED_METRIC_NAMES.len() {
        return Err(format!(
            "P2 quality report contains {} metrics; expected {}",
            report.metrics().len(),
            EXPECTED_METRIC_NAMES.len()
        ));
    }
    for (metric, expected_name) in report.metrics().iter().zip(EXPECTED_METRIC_NAMES) {
        if metric.id().namespace() != METRIC_NAMESPACE
            || metric.id().version() != METRIC_VERSION
            || metric.id().name() != expected_name
        {
            return Err(format!(
                "unexpected P2 metric {}.{}.v{}; expected {}.{}.v{}",
                metric.id().namespace(),
                metric.id().name(),
                metric.id().version(),
                METRIC_NAMESPACE,
                expected_name,
                METRIC_VERSION
            ));
        }
        if !CORPUS_SCOPED_METRICS.contains(&expected_name)
            && metric.status() != QualityMetricStatus::Pass
        {
            return Err(format!(
                "per-world P2 metric {}.{}.v{} returned {:?}",
                metric.id().namespace(),
                metric.id().name(),
                metric.id().version(),
                metric.status()
            ));
        }
    }
    Ok(())
}

fn metric_id(name: &str) -> Result<QualityMetricId, QualityBuildError> {
    Ok(QualityMetricId::new(
        METRIC_NAMESPACE,
        name,
        METRIC_VERSION,
    )?)
}

fn invalid_input(input: &'static str, reason: String) -> QualityBuildError {
    QualityBuildError::InvalidInput { input, reason }
}

fn count(found: usize, field: &'static str) -> Result<u32, QualityBuildError> {
    u32::try_from(found).map_err(|_| QualityBuildError::CountOverflow { field, found })
}

fn checked_ratio(
    numerator: f64,
    denominator: f64,
    input: &'static str,
) -> Result<f64, QualityBuildError> {
    let value = numerator / denominator;
    if denominator <= 0.0 || !value.is_finite() {
        return Err(invalid_input(
            input,
            "ratio requires a positive finite denominator and finite result".to_owned(),
        ));
    }
    Ok(value)
}

fn maximum_plate_area_fraction(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &EvolvedTectonicSnapshot,
) -> Result<f64, QualityBuildError> {
    let tectonic = snapshot.compatibility();
    let mut areas = vec![0.0_f64; tectonic.plates().len()];
    for (index, cell) in surface.cells().iter().enumerate() {
        let owner = tectonic
            .cell_plates()
            .get(index)
            .ok_or_else(|| invalid_input("cell-plates", format!("missing cell {index}")))?;
        let slot = areas.get_mut(owner.raw() as usize).ok_or_else(|| {
            invalid_input("cell-plates", format!("unknown plate {}", owner.raw()))
        })?;
        *slot += cell.area.get();
    }
    let total = surface.total_cell_area().get();
    checked_ratio(
        areas.into_iter().fold(0.0_f64, f64::max),
        total,
        "maximum-plate-area-fraction",
    )
}

struct BoundaryCausality {
    subduction: MetricObservation,
    collision: MetricObservation,
    transform_ratio: MetricObservation,
}

fn boundary_causality(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &EvolvedTectonicSnapshot,
) -> Result<BoundaryCausality, QualityBuildError> {
    let tectonic = snapshot.compatibility();
    let forcing = snapshot.forcing();
    let uplift = forcing.uplift_rate_mm_per_year();
    let subsidence = forcing.subsidence_rate_mm_per_year();
    let shortening = forcing.shortening_rate_mm_per_year();
    let mut subduction_total = 0_usize;
    let mut subduction_passed = 0_usize;
    let mut collision_total = 0_usize;
    let mut collision_passed = 0_usize;
    let mut transform_uplift = Vec::new();
    let mut convergent_uplift = Vec::new();

    for (edge, boundary) in surface.edges().iter().zip(tectonic.boundaries()) {
        let [first, second] = edge.cells.map(|cell| cell.raw() as usize);
        match boundary.kind {
            BoundaryKind::Subduction => {
                let descending = boundary.subducting_plate.ok_or_else(|| {
                    invalid_input("subduction", "missing descending plate".to_owned())
                })?;
                let first_owner = tectonic
                    .cell_plates()
                    .get(first)
                    .ok_or_else(|| invalid_input("cell-plates", format!("missing cell {first}")))?;
                let (descending_cell, overriding_cell) = if first_owner == descending {
                    (first, second)
                } else {
                    (second, first)
                };
                subduction_total += 1;
                subduction_passed +=
                    usize::from(subsidence[descending_cell] > 0.0 && uplift[overriding_cell] > 0.0);
                convergent_uplift.push(uplift[overriding_cell].abs());
            }
            BoundaryKind::ContinentalCollision => {
                collision_total += 1;
                collision_passed += usize::from(
                    shortening[first] > 0.0
                        && shortening[second] > 0.0
                        && uplift[first] > 0.0
                        && uplift[second] > 0.0,
                );
                convergent_uplift.extend([uplift[first].abs(), uplift[second].abs()]);
            }
            BoundaryKind::Transform => {
                transform_uplift.extend([uplift[first].abs(), uplift[second].abs()]);
            }
            BoundaryKind::None
            | BoundaryKind::Weak
            | BoundaryKind::ContinentalRift
            | BoundaryKind::OceanicRidge => {}
        }
    }

    let subduction = fraction_observation(
        subduction_passed,
        subduction_total,
        "no ocean-continent subduction edges in this world",
    )?;
    let collision = fraction_observation(
        collision_passed,
        collision_total,
        "no continental-collision edges in this world",
    )?;
    let transform_ratio = if transform_uplift.is_empty() {
        MetricObservation::Unavailable {
            reason: "no transform edges in this world".to_owned(),
        }
    } else if convergent_uplift.is_empty() {
        MetricObservation::Unavailable {
            reason: "no convergent edges in this world".to_owned(),
        }
    } else {
        let transform = median(&mut transform_uplift);
        let convergent = median(&mut convergent_uplift);
        if convergent <= 0.0 {
            MetricObservation::Unavailable {
                reason: "convergent median uplift is zero".to_owned(),
            }
        } else {
            MetricObservation::Available {
                value: checked_ratio(transform, convergent, "transform-uplift-ratio")?,
                sample_count: count(
                    transform_uplift.len() + convergent_uplift.len(),
                    "transform-uplift-ratio",
                )?,
            }
        }
    };
    Ok(BoundaryCausality {
        subduction,
        collision,
        transform_ratio,
    })
}

fn append_boundary_uplift_samples(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &EvolvedTectonicSnapshot,
    transform_uplift: &mut Vec<f32>,
    convergent_uplift: &mut Vec<f32>,
) -> Result<(), QualityBuildError> {
    let tectonic = snapshot.compatibility();
    let uplift = snapshot.forcing().uplift_rate_mm_per_year();
    for (edge, boundary) in surface.edges().iter().zip(tectonic.boundaries()) {
        let [first, second] = edge.cells.map(|cell| cell.raw() as usize);
        match boundary.kind {
            BoundaryKind::Subduction => {
                let descending = boundary.subducting_plate.ok_or_else(|| {
                    invalid_input("subduction", "missing descending plate".to_owned())
                })?;
                let first_owner = tectonic
                    .cell_plates()
                    .get(first)
                    .ok_or_else(|| invalid_input("cell-plates", format!("missing cell {first}")))?;
                let overriding = if first_owner == descending {
                    second
                } else {
                    first
                };
                convergent_uplift.push(uplift[overriding].abs());
            }
            BoundaryKind::ContinentalCollision => {
                convergent_uplift.extend([uplift[first].abs(), uplift[second].abs()]);
            }
            BoundaryKind::Transform => {
                transform_uplift.extend([uplift[first].abs(), uplift[second].abs()]);
            }
            BoundaryKind::None
            | BoundaryKind::Weak
            | BoundaryKind::ContinentalRift
            | BoundaryKind::OceanicRidge => {}
        }
    }
    Ok(())
}

fn fraction_observation(
    passed: usize,
    total: usize,
    empty_reason: &'static str,
) -> Result<MetricObservation, QualityBuildError> {
    if total == 0 {
        Ok(MetricObservation::Unavailable {
            reason: empty_reason.to_owned(),
        })
    } else {
        Ok(MetricObservation::Available {
            value: passed as f64 / total as f64,
            sample_count: count(total, "event-causality")?,
        })
    }
}

fn ocean_age_depth_spearman(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &EvolvedTectonicSnapshot,
) -> Result<MetricObservation, QualityBuildError> {
    let mut values = Vec::new();
    append_ocean_age_depth(surface, snapshot, &mut values);
    if values.len() < 2 {
        return Ok(MetricObservation::Unavailable {
            reason: "fewer than two oceanic cells".to_owned(),
        });
    }
    let Some(value) = weighted_spearman(&values) else {
        return Ok(MetricObservation::Unavailable {
            reason: "ocean age or depth ranks have zero weighted variance".to_owned(),
        });
    };
    if !value.is_finite() {
        return Err(QualityBuildError::NonFiniteAccumulation);
    }
    Ok(MetricObservation::Available {
        value,
        sample_count: count(values.len(), "ocean-age-depth-spearman")?,
    })
}

fn append_ocean_age_depth(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &EvolvedTectonicSnapshot,
    values: &mut Vec<(f64, f64, f64)>,
) {
    let tectonic = snapshot.compatibility();
    for (index, cell) in surface.cells().iter().enumerate() {
        if tectonic.crust_kinds().get(index) == Some(CrustKind::Oceanic) {
            values.push((
                f64::from(tectonic.crust_age_myr()[index]),
                -f64::from(tectonic.tectonic_elevation_m()[index]),
                cell.area.get(),
            ));
        }
    }
}

fn regular_triple_junction_fraction(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &EvolvedTectonicSnapshot,
) -> Result<MetricObservation, QualityBuildError> {
    let tectonic = snapshot.compatibility();
    let mut incident = vec![Vec::new(); surface.vertices().len()];
    for edge in surface.edges() {
        if plate_pair(surface, tectonic, edge.id).is_some() {
            for vertex in edge.vertices {
                incident[vertex.raw() as usize].push(edge.id);
            }
        }
    }
    let mut angles = Vec::new();
    for vertex in surface.vertices() {
        let edges = &incident[vertex.id.raw() as usize];
        let owners = edges
            .iter()
            .flat_map(|&edge| {
                plate_pair(surface, tectonic, edge)
                    .expect("incident inventory contains only plate boundaries")
            })
            .collect::<BTreeSet<_>>();
        if owners.len() != 3 || edges.len() != 3 {
            continue;
        }
        let (east, north) = canonical_east_north_basis(vertex.position);
        let mut azimuths = edges
            .iter()
            .filter_map(|&edge| {
                let endpoint = trace_plate_branch(
                    surface,
                    tectonic,
                    &incident,
                    vertex.id,
                    edge,
                    MACRO_BRANCH_LENGTH_M,
                );
                let tangent = project_tangent(
                    surface.vertex(endpoint)?.position.components(),
                    vertex.position,
                );
                let length = dot(tangent, tangent).sqrt();
                (length > f64::EPSILON).then(|| {
                    let direction = tangent.map(|component| component / length);
                    dot(direction, north).atan2(dot(direction, east))
                })
            })
            .collect::<Vec<_>>();
        if azimuths.len() != 3 {
            continue;
        }
        azimuths.sort_by(f64::total_cmp);
        for index in 0..3 {
            let next = if index == 2 {
                azimuths[0] + 2.0 * PI
            } else {
                azimuths[index + 1]
            };
            angles.push((next - azimuths[index]).to_degrees());
        }
    }
    if angles.is_empty() {
        return Ok(MetricObservation::Unavailable {
            reason: "no three-lineage macro junctions in this world".to_owned(),
        });
    }
    let regular = angles
        .iter()
        .filter(|&&angle| (angle - 120.0).abs() <= 10.0)
        .count();
    Ok(MetricObservation::Available {
        value: regular as f64 / angles.len() as f64,
        sample_count: count(angles.len(), "triple-junction-angles")?,
    })
}

fn plate_pair(
    surface: &SphericalSurfaceSnapshot,
    tectonic: &crate::world::natural::SphericalTectonicSnapshot,
    edge: EdgeId,
) -> Option<[PlateId; 2]> {
    let cells = surface.edge(edge)?.cells;
    let mut pair = [
        tectonic.plate_for_cell(cells[0])?,
        tectonic.plate_for_cell(cells[1])?,
    ];
    if pair[0] == pair[1] {
        None
    } else {
        if pair[1] < pair[0] {
            pair.swap(0, 1);
        }
        Some(pair)
    }
}

fn trace_plate_branch(
    surface: &SphericalSurfaceSnapshot,
    tectonic: &crate::world::natural::SphericalTectonicSnapshot,
    incident: &[Vec<EdgeId>],
    start: SurfaceVertexId,
    first_edge: EdgeId,
    target_length_m: f64,
) -> SurfaceVertexId {
    let pair =
        plate_pair(surface, tectonic, first_edge).expect("macro branch starts on a plate boundary");
    let mut previous_vertex = start;
    let mut edge = first_edge;
    let mut length = 0.0;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(edge) {
            return previous_vertex;
        }
        let record = surface
            .edge(edge)
            .expect("validated incident edge belongs to the surface");
        let next_vertex = if record.vertices[0] == previous_vertex {
            record.vertices[1]
        } else {
            record.vertices[0]
        };
        length += record.length.get();
        if length >= target_length_m {
            return next_vertex;
        }
        let candidates = incident[next_vertex.raw() as usize]
            .iter()
            .copied()
            .filter(|&candidate| candidate != edge)
            .filter(|&candidate| plate_pair(surface, tectonic, candidate) == Some(pair))
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return next_vertex;
        }
        previous_vertex = next_vertex;
        edge = candidates[0];
    }
}

fn median(values: &mut [f32]) -> f64 {
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (f64::from(values[middle - 1]) + f64::from(values[middle])) * 0.5
    } else {
        f64::from(values[middle])
    }
}

fn median_f64(values: &[f64]) -> f64 {
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}

fn weighted_spearman(values: &[(f64, f64, f64)]) -> Option<f64> {
    let first = average_ranks(values, |value| value.0);
    let second = average_ranks(values, |value| value.1);
    let weight_sum = values.iter().map(|value| value.2).sum::<f64>();
    if !weight_sum.is_finite() || weight_sum <= 0.0 {
        return None;
    }
    let first_mean = first
        .iter()
        .zip(values)
        .map(|(rank, value)| rank * value.2)
        .sum::<f64>()
        / weight_sum;
    let second_mean = second
        .iter()
        .zip(values)
        .map(|(rank, value)| rank * value.2)
        .sum::<f64>()
        / weight_sum;
    let mut covariance = 0.0;
    let mut first_variance = 0.0;
    let mut second_variance = 0.0;
    for index in 0..values.len() {
        let first_delta = first[index] - first_mean;
        let second_delta = second[index] - second_mean;
        let weight = values[index].2;
        covariance += weight * first_delta * second_delta;
        first_variance += weight * first_delta * first_delta;
        second_variance += weight * second_delta * second_delta;
    }
    let scale = (first_variance * second_variance).sqrt();
    (scale > 0.0).then_some(covariance / scale)
}

fn average_ranks(values: &[(f64, f64, f64)], key: impl Fn(&(f64, f64, f64)) -> f64) -> Vec<f64> {
    let mut order = (0..values.len()).collect::<Vec<_>>();
    order.sort_by(|&first, &second| key(&values[first]).total_cmp(&key(&values[second])));
    let mut ranks = vec![0.0; values.len()];
    let mut start = 0;
    while start < order.len() {
        let mut end = start + 1;
        while end < order.len()
            && key(&values[order[end]]).to_bits() == key(&values[order[start]]).to_bits()
        {
            end += 1;
        }
        let average = (start + end - 1) as f64 * 0.5;
        for &index in &order[start..end] {
            ranks[index] = average;
        }
        start = end;
    }
    ranks
}

fn non_finite_count(snapshot: &EvolvedTectonicSnapshot) -> (usize, usize) {
    let material = snapshot.material();
    let forcing = snapshot.forcing();
    let tectonic = snapshot.compatibility();
    let f64_fields = [
        material.continental_reference_area_m2(),
        material.continental_volume_m3(),
        material.oceanic_reference_area_m2(),
        material.oceanic_volume_m3(),
    ];
    let f32_fields = [
        forcing.uplift_rate_mm_per_year(),
        forcing.subsidence_rate_mm_per_year(),
        forcing.shortening_rate_mm_per_year(),
        forcing.boundary_distance_m(),
        forcing.event_age_myr(),
        tectonic.crust_thickness_km(),
        tectonic.crust_age_myr(),
        tectonic.tectonic_elevation_m(),
        tectonic.lineation_east(),
        tectonic.lineation_north(),
        tectonic.orogeny_age_myr(),
    ];
    let inspected = f64_fields.iter().map(|values| values.len()).sum::<usize>()
        + f32_fields.iter().map(|values| values.len()).sum::<usize>();
    let non_finite = f64_fields
        .iter()
        .flat_map(|values| values.iter())
        .filter(|value| !value.is_finite())
        .count()
        + f32_fields
            .iter()
            .flat_map(|values| values.iter())
            .filter(|value| !value.is_finite())
            .count();
    (non_finite, inspected)
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first.into_iter().zip(second).map(|(a, b)| a * b).sum()
}
