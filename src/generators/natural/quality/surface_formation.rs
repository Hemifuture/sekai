//! Locked P5 conservation, causality, and formation-morphology quality gates.

use super::{MetricObservation, NaturalQualityReportBuilder, QualityBuildError};
use crate::engine::BuildCancellation;
use crate::world::natural::{
    formation_elevation_from_components, LandOceanKind, NaturalQualityProfile,
    NaturalQualityReport, NaturalSurfaceFormationSnapshot, PrimaryReliefSnapshot, QualityMetricId,
    QualityMetricStatus, SurfaceWaterKind, FORMATION_SHELF_BREAK_DEPTH_M,
    SEDIMENT_BUDGET_RELATIVE_ERROR_MAX, SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX,
    WATER_VOLUME_RELATIVE_TOLERANCE,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use crate::world::CellId;

const METRIC_NAMESPACE: &str = "sekai.surface-formation-v1";
const METRIC_VERSION: u16 = 1;
const CANCELLATION_POLL_MASK: usize = 255;
const CENTIMETERS_PER_METER: f64 = 100.0;

/// Land-area fraction below which a world cannot be asked for a deep network.
const NETWORK_LAND_FRACTION_MIN: f64 = 0.10;
const NO_LAND_REASON: &str = "the published world has no dry land";
const NO_NETWORK_LAND_REASON: &str = "dry land covers at most 10% of the published world";
const NO_DEPOSIT_REASON: &str = "the solve deposited no sediment anywhere";
const NO_INCISION_REASON: &str = "the solve produced no fluvial incision";

const EXPECTED_METRIC_NAMES: [&str; 14] = [
    "component-identity-mismatch-count",
    "deposited-sediment-enrichment-ratio",
    "final-land-fraction-absolute-change",
    "fixed-point-normalized-residual",
    "fluvial-incision-support-enrichment-ratio",
    "land-outlet-path-area-fraction",
    "largest-network-strahler-order",
    "primary-final-elevation-correlation",
    "provenance-mass-relative-error",
    "receiver-adjacency-violation-count",
    "river-reach-count",
    "sediment-mass-relative-error",
    "through-ocean-land-river-count",
    "water-volume-relative-error",
];

/// Returns the locked per-profile bounds in the canonical metric order.
fn expected_metric_bounds(profile: NaturalQualityProfile) -> [(Option<f64>, Option<f64>); 14] {
    let strahler_min = match profile {
        NaturalQualityProfile::Draft => 3.0,
        NaturalQualityProfile::Standard | NaturalQualityProfile::High => 4.0,
    };
    [
        (None, Some(0.0)),
        (Some(1.25), None),
        (None, Some(0.03)),
        (None, Some(1.0)),
        (Some(1.50), None),
        (Some(0.95), None),
        (Some(strahler_min), None),
        (Some(0.90), None),
        (None, Some(SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX)),
        (None, Some(0.0)),
        (Some(1.0), None),
        (None, Some(SEDIMENT_BUDGET_RELATIVE_ERROR_MAX)),
        (None, Some(0.0)),
        (None, Some(WATER_VOLUME_RELATIVE_TOLERANCE)),
    ]
}

/// Evaluates every locked P5 gate against one published formation product.
pub fn evaluate_surface_formation_quality(
    surface: &SphericalSurfaceSnapshot,
    relief: &PrimaryReliefSnapshot,
    snapshot: &NaturalSurfaceFormationSnapshot,
) -> Result<NaturalQualityReport, QualityBuildError> {
    evaluate_impl(surface, relief, snapshot, None)
}

/// Cancellation-aware production path used by the atomic P5 stage.
pub fn evaluate_surface_formation_quality_cancellable(
    surface: &SphericalSurfaceSnapshot,
    relief: &PrimaryReliefSnapshot,
    snapshot: &NaturalSurfaceFormationSnapshot,
    cancellation: &BuildCancellation,
) -> Result<NaturalQualityReport, QualityBuildError> {
    evaluate_impl(surface, relief, snapshot, Some(cancellation))
}

fn evaluate_impl(
    surface: &SphericalSurfaceSnapshot,
    relief: &PrimaryReliefSnapshot,
    snapshot: &NaturalSurfaceFormationSnapshot,
    cancellation: Option<&BuildCancellation>,
) -> Result<NaturalQualityReport, QualityBuildError> {
    let surface_ref = SurfaceRef::from_validated_spherical(surface).map_err(|error| {
        QualityBuildError::InvalidInput {
            input: "authoritative_surface",
            reason: error.to_string(),
        }
    })?;
    if snapshot.surface_ref() != surface_ref {
        return Err(QualityBuildError::SurfaceMismatch {
            input: "natural_surface_formation",
            found: snapshot.surface_ref(),
            expected: surface_ref,
        });
    }
    if relief.surface_ref() != surface_ref {
        return Err(QualityBuildError::InvalidInput {
            input: "primary_relief",
            reason: "primary relief belongs to a different authoritative surface".to_owned(),
        });
    }

    if relief.elevation_m()
        != snapshot
            .terrain_fields()
            .elevation_components()
            .primary_elevation_m()
        || relief.water_inventory_m3() != snapshot.terrain_fields().water_inventory_m3()
    {
        return Err(QualityBuildError::InvalidInput {
            input: "primary_relief",
            reason: "primary relief is not the immutable terrain this product was solved from"
                .to_owned(),
        });
    }

    let state = FormationQualityState::collect(surface, relief, snapshot, cancellation)?;
    let mut builder = NaturalQualityReportBuilder::new(surface_ref);
    let bounds = expected_metric_bounds(snapshot.checkpoint().quality_profile());
    let cells = surface.cells().len() as u32;

    builder.record_at_most(
        metric_id(EXPECTED_METRIC_NAMES[0])?,
        state.component_identity_mismatch_count,
        cells,
        bounds[0].1.expect("locked maximum"),
    )?;
    builder.record_observation_at_least(
        metric_id(EXPECTED_METRIC_NAMES[1])?,
        state.deposited_sediment_enrichment(),
        bounds[1].0.expect("locked minimum"),
    )?;
    builder.record_at_most(
        metric_id(EXPECTED_METRIC_NAMES[2])?,
        state.land_fraction_absolute_change,
        cells,
        bounds[2].1.expect("locked maximum"),
    )?;
    builder.record_at_most(
        metric_id(EXPECTED_METRIC_NAMES[3])?,
        snapshot.solve_report().final_residual().normalized_max(),
        u32::from(snapshot.solve_report().outer_iterations()),
        bounds[3].1.expect("locked maximum"),
    )?;
    builder.record_observation_at_least(
        metric_id(EXPECTED_METRIC_NAMES[4])?,
        state.fluvial_incision_enrichment(),
        bounds[4].0.expect("locked minimum"),
    )?;
    builder.record_observation_at_least(
        metric_id(EXPECTED_METRIC_NAMES[5])?,
        state.land_outlet_path_fraction(),
        bounds[5].0.expect("locked minimum"),
    )?;
    builder.record_observation_at_least(
        metric_id(EXPECTED_METRIC_NAMES[6])?,
        state.largest_network_strahler_order(),
        bounds[6].0.expect("locked minimum"),
    )?;
    builder.record_observation_at_least(
        metric_id(EXPECTED_METRIC_NAMES[7])?,
        available(state.primary_final_correlation, cells),
        bounds[7].0.expect("locked minimum"),
    )?;
    builder.record_at_most(
        metric_id(EXPECTED_METRIC_NAMES[8])?,
        state.provenance_relative_error,
        cells,
        bounds[8].1.expect("locked maximum"),
    )?;
    builder.record_at_most(
        metric_id(EXPECTED_METRIC_NAMES[9])?,
        state.receiver_adjacency_violation_count,
        cells,
        bounds[9].1.expect("locked maximum"),
    )?;
    builder.record_observation_at_least(
        metric_id(EXPECTED_METRIC_NAMES[10])?,
        state.river_reach_count(),
        bounds[10].0.expect("locked minimum"),
    )?;
    builder.record_at_most(
        metric_id(EXPECTED_METRIC_NAMES[11])?,
        snapshot.sediment_budget_report().global_relative_error(),
        cells,
        bounds[11].1.expect("locked maximum"),
    )?;
    builder.record_at_most(
        metric_id(EXPECTED_METRIC_NAMES[12])?,
        state.through_ocean_land_river_count,
        cells,
        bounds[12].1.expect("locked maximum"),
    )?;
    builder.record_at_most(
        metric_id(EXPECTED_METRIC_NAMES[13])?,
        state.water_volume_relative_error,
        cells,
        bounds[13].1.expect("locked maximum"),
    )?;

    Ok(builder
        .finish()?
        .bind_subject_fingerprint(*snapshot.checkpoint().fingerprint())?)
}

/// Every dense reduction the locked gates share, collected in one pass set.
struct FormationQualityState {
    component_identity_mismatch_count: f64,
    land_fraction_absolute_change: f64,
    primary_final_correlation: f64,
    provenance_relative_error: f64,
    receiver_adjacency_violation_count: f64,
    through_ocean_land_river_count: f64,
    water_volume_relative_error: f64,
    land_area_m2: f64,
    total_area_m2: f64,
    outlet_land_area_m2: f64,
    deposit_volume_m3: f64,
    eligible_deposit_volume_m3: f64,
    eligible_area_m2: f64,
    incision_volume_m3: f64,
    support_incision_volume_m3: f64,
    support_area_m2: f64,
    largest_strahler_order: u32,
    river_reach_count: usize,
    land_cell_count: u32,
    eligible_cell_count: u32,
    support_cell_count: u32,
}

impl FormationQualityState {
    fn collect(
        surface: &SphericalSurfaceSnapshot,
        relief: &PrimaryReliefSnapshot,
        snapshot: &NaturalSurfaceFormationSnapshot,
        cancellation: Option<&BuildCancellation>,
    ) -> Result<Self, QualityBuildError> {
        let terrain = snapshot.terrain_fields();
        let components = terrain.elevation_components();
        let hydrology = snapshot.hydrology();
        let sediment = terrain.sediment();
        let elevation = terrain.final_elevation_m();
        let drainage = hydrology.drainage_surface_elevation_m().values();
        let discharge = hydrology.mean_annual_discharge_m3_s();
        let count = surface.cells().len();

        let mut state = Self {
            component_identity_mismatch_count: 0.0,
            land_fraction_absolute_change: 0.0,
            primary_final_correlation: 0.0,
            provenance_relative_error: snapshot
                .sediment_budget_report()
                .provenance_relative_errors()
                .iter()
                .copied()
                .fold(0.0_f64, f64::max),
            receiver_adjacency_violation_count: 0.0,
            through_ocean_land_river_count: 0.0,
            water_volume_relative_error: relative_error(
                terrain.realized_water_volume_m3(),
                terrain.water_inventory_m3(),
            ),
            land_area_m2: 0.0,
            total_area_m2: 0.0,
            outlet_land_area_m2: 0.0,
            deposit_volume_m3: 0.0,
            eligible_deposit_volume_m3: 0.0,
            eligible_area_m2: 0.0,
            incision_volume_m3: 0.0,
            support_incision_volume_m3: 0.0,
            support_area_m2: 0.0,
            largest_strahler_order: 0,
            river_reach_count: hydrology.river_segments().len(),
            land_cell_count: 0,
            eligible_cell_count: 0,
            support_cell_count: 0,
        };

        let outlet_paths = resolve_outlet_paths(hydrology.flow_receiver());
        let mut relief_land_area_m2 = 0.0_f64;
        let mut primary_sum = 0.0_f64;
        let mut final_sum = 0.0_f64;
        let mut land_samples: Vec<(f64, f64, usize)> = Vec::new();
        for index in 0..count {
            poll_cancelled(cancellation, index)?;
            let area_m2 = surface.cells()[index].area.get();
            state.total_area_m2 += area_m2;
            primary_sum += area_m2 * f64::from(components.primary_elevation_m()[index]);
            final_sum += area_m2 * f64::from(elevation[index]);

            let expected = formation_elevation_from_components(
                components.primary_elevation_m()[index],
                components.tectonic_displacement_m()[index],
                components.fluvial_erosion_m()[index],
                components.hillslope_erosion_m()[index],
                components.hillslope_deposition_m()[index],
                components.routed_sediment_deposition_m()[index],
                components.coastal_erosion_m()[index],
                components.coastal_deposition_m()[index],
                components.isostatic_response_m()[index],
            );
            if elevation[index].to_bits() != expected.to_bits() {
                state.component_identity_mismatch_count += 1.0;
            }

            if relief.land_ocean().raw_values()[index] == LandOceanKind::Land.raw() {
                relief_land_area_m2 += area_m2;
            }
            let water = hydrology.surface_water().get(index);
            let is_land = terrain.land_ocean().raw_values()[index] == LandOceanKind::Land.raw();
            if is_land {
                state.land_area_m2 += area_m2;
                state.land_cell_count += 1;
                if outlet_paths[index] && hydrology.basin_id()[index].is_some() {
                    state.outlet_land_area_m2 += area_m2;
                }
            }

            let deposit_m3 = f64::from(sediment.sediment_thickness_m()[index]) * area_m2;
            state.deposit_volume_m3 += deposit_m3;
            let water_depth_m = f64::from(terrain.sea_level_m()) - f64::from(elevation[index]);
            let eligible = match water {
                Some(SurfaceWaterKind::Lake) => true,
                Some(SurfaceWaterKind::Ocean) => {
                    water_depth_m <= FORMATION_SHELF_BREAK_DEPTH_M
                        || f64::from(sediment.delta_potential()[index]) > 0.0
                }
                _ => hydrology.flow_receiver()[index].is_none(),
            };
            if eligible {
                state.eligible_deposit_volume_m3 += deposit_m3;
                state.eligible_area_m2 += area_m2;
                state.eligible_cell_count += 1;
            }

            let incision_m3 = f64::from(components.fluvial_erosion_m()[index]) * area_m2;
            if is_land {
                state.incision_volume_m3 += incision_m3;
                let slope = receiver_slope(surface, hydrology.flow_receiver(), drainage, index);
                land_samples.push((f64::from(discharge[index]), slope, index));
            }

            if let Some(receiver) = hydrology.flow_receiver()[index] {
                if !is_adjacent(surface, index, receiver)
                    || !drains_downhill(drainage, index, receiver)
                {
                    state.receiver_adjacency_violation_count += 1.0;
                }
                if water == Some(SurfaceWaterKind::Ocean) {
                    state.through_ocean_land_river_count += 1.0;
                }
            }
            state.largest_strahler_order = state
                .largest_strahler_order
                .max(hydrology.strahler_order().raw_values()[index]);
        }

        for (position, segment) in hydrology.river_segments().iter().enumerate() {
            poll_cancelled(cancellation, position)?;
            let from = segment.from().raw() as usize;
            if hydrology.surface_water().get(from) == Some(SurfaceWaterKind::Ocean) {
                state.through_ocean_land_river_count += 1.0;
            }
        }

        let final_land_fraction = state.land_area_m2 / state.total_area_m2;
        let relief_land_fraction = relief_land_area_m2 / state.total_area_m2;
        state.land_fraction_absolute_change = (final_land_fraction - relief_land_fraction).abs();
        state.primary_final_correlation = area_weighted_correlation(
            surface,
            components.primary_elevation_m(),
            elevation,
            primary_sum / state.total_area_m2,
            final_sum / state.total_area_m2,
            cancellation,
        )?;

        if !land_samples.is_empty() {
            let discharge_threshold = weighted_median(&mut land_samples, |sample| sample.0);
            let slope_threshold = weighted_median(&mut land_samples, |sample| sample.1);
            for (position, &(cell_discharge, cell_slope, index)) in land_samples.iter().enumerate()
            {
                poll_cancelled(cancellation, position)?;
                if cell_discharge >= discharge_threshold && cell_slope >= slope_threshold {
                    let area_m2 = surface.cells()[index].area.get();
                    state.support_area_m2 += area_m2;
                    state.support_cell_count += 1;
                    state.support_incision_volume_m3 +=
                        f64::from(components.fluvial_erosion_m()[index]) * area_m2;
                }
            }
        }
        Ok(state)
    }

    fn deposited_sediment_enrichment(&self) -> MetricObservation {
        if self.deposit_volume_m3 <= 0.0 {
            return unavailable(NO_DEPOSIT_REASON);
        }
        if self.eligible_area_m2 <= 0.0 {
            return unavailable(NO_LAND_REASON);
        }
        let deposit_share = self.eligible_deposit_volume_m3 / self.deposit_volume_m3;
        let area_share = self.eligible_area_m2 / self.total_area_m2;
        available(deposit_share / area_share, self.eligible_cell_count)
    }

    fn fluvial_incision_enrichment(&self) -> MetricObservation {
        if self.land_area_m2 <= 0.0 {
            return unavailable(NO_LAND_REASON);
        }
        if self.incision_volume_m3 <= 0.0 || self.support_area_m2 <= 0.0 {
            return unavailable(NO_INCISION_REASON);
        }
        let incision_share = self.support_incision_volume_m3 / self.incision_volume_m3;
        let area_share = self.support_area_m2 / self.land_area_m2;
        available(incision_share / area_share, self.support_cell_count)
    }

    fn land_outlet_path_fraction(&self) -> MetricObservation {
        if self.land_area_m2 <= 0.0 {
            return unavailable(NO_LAND_REASON);
        }
        available(
            self.outlet_land_area_m2 / self.land_area_m2,
            self.land_cell_count,
        )
    }

    fn largest_network_strahler_order(&self) -> MetricObservation {
        if self.land_area_m2 / self.total_area_m2 <= NETWORK_LAND_FRACTION_MIN {
            return unavailable(NO_NETWORK_LAND_REASON);
        }
        available(
            f64::from(self.largest_strahler_order),
            self.river_reach_count.min(u32::MAX as usize) as u32,
        )
    }

    fn river_reach_count(&self) -> MetricObservation {
        if self.land_area_m2 <= 0.0 {
            return unavailable(NO_LAND_REASON);
        }
        available(
            self.river_reach_count as f64,
            self.river_reach_count.min(u32::MAX as usize) as u32,
        )
    }
}

fn receiver_slope(
    surface: &SphericalSurfaceSnapshot,
    receivers: &[Option<CellId>],
    drainage: &[f32],
    index: usize,
) -> f64 {
    let Some(receiver) = receivers[index] else {
        return 0.0;
    };
    let cell = CellId::from_raw(index as u32);
    let Some(length_m) = center_distance_m(surface, cell, receiver) else {
        return 0.0;
    };
    if length_m <= 0.0 {
        return 0.0;
    }
    ((f64::from(drainage[index]) - f64::from(drainage[receiver.raw() as usize])) / length_m)
        .max(0.0)
}

fn center_distance_m(
    surface: &SphericalSurfaceSnapshot,
    cell: CellId,
    receiver: CellId,
) -> Option<f64> {
    surface.cell_edges(cell).and_then(|edges| {
        edges.iter().find_map(|&edge| {
            (surface.opposite_cell(cell, edge) == Some(receiver))
                .then(|| {
                    surface
                        .edge(edge)
                        .map(|record| record.center_distance.get())
                })
                .flatten()
        })
    })
}

fn is_adjacent(surface: &SphericalSurfaceSnapshot, index: usize, receiver: CellId) -> bool {
    center_distance_m(surface, CellId::from_raw(index as u32), receiver).is_some()
}

/// A receiver may never sit above its donor on the centimetre-quantized filled
/// drainage surface. Equal-height flats are resolved by the solver's own flood
/// dequeue rank, which is not part of the published product, so this gate
/// checks the published monotonicity instead of reconstructing that rank.
fn drains_downhill(drainage: &[f32], index: usize, receiver: CellId) -> bool {
    drainage_key_cm(drainage[receiver.raw() as usize]) <= drainage_key_cm(drainage[index])
}

/// Marks every cell whose receiver chain reaches a real terminal. A chain that
/// re-enters itself is a cycle and can never reach an outlet.
fn resolve_outlet_paths(receivers: &[Option<CellId>]) -> Vec<bool> {
    const UNKNOWN: u8 = 0;
    const PENDING: u8 = 1;
    const REACHES: u8 = 2;
    const CYCLIC: u8 = 3;

    let mut state = vec![UNKNOWN; receivers.len()];
    let mut chain = Vec::new();
    for start in 0..receivers.len() {
        if state[start] != UNKNOWN {
            continue;
        }
        let mut cell = start;
        let resolved = loop {
            match state[cell] {
                UNKNOWN => {
                    state[cell] = PENDING;
                    chain.push(cell);
                    match receivers[cell] {
                        Some(next) => cell = next.raw() as usize,
                        None => break REACHES,
                    }
                }
                PENDING => break CYCLIC,
                REACHES => break REACHES,
                _ => break CYCLIC,
            }
        };
        for cell in chain.drain(..) {
            state[cell] = resolved;
        }
    }
    state.into_iter().map(|value| value == REACHES).collect()
}

fn drainage_key_cm(height_m: f32) -> i64 {
    (f64::from(height_m) * CENTIMETERS_PER_METER).round() as i64
}

fn area_weighted_correlation(
    surface: &SphericalSurfaceSnapshot,
    first: &[f32],
    second: &[f32],
    first_mean: f64,
    second_mean: f64,
    cancellation: Option<&BuildCancellation>,
) -> Result<f64, QualityBuildError> {
    let mut covariance = 0.0_f64;
    let mut first_variance = 0.0_f64;
    let mut second_variance = 0.0_f64;
    for (index, cell) in surface.cells().iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        let area_m2 = cell.area.get();
        let first_delta = f64::from(first[index]) - first_mean;
        let second_delta = f64::from(second[index]) - second_mean;
        covariance += area_m2 * first_delta * second_delta;
        first_variance += area_m2 * first_delta * first_delta;
        second_variance += area_m2 * second_delta * second_delta;
    }
    let denominator = (first_variance * second_variance).sqrt();
    if denominator <= 0.0 {
        return Ok(1.0);
    }
    Ok((covariance / denominator).clamp(-1.0, 1.0))
}

/// Returns the area-independent median of one land sample projection.
fn weighted_median(
    samples: &mut [(f64, f64, usize)],
    project: impl Fn(&(f64, f64, usize)) -> f64,
) -> f64 {
    let mut values = samples.iter().map(&project).collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn relative_error(found: f64, expected: f64) -> f64 {
    let scale = found.abs().max(expected.abs());
    if scale == 0.0 {
        0.0
    } else {
        (found - expected).abs() / scale
    }
}

fn available(value: f64, sample_count: u32) -> MetricObservation {
    MetricObservation::Available {
        value,
        sample_count: sample_count.max(1),
    }
}

fn unavailable(reason: &str) -> MetricObservation {
    MetricObservation::Unavailable {
        reason: reason.to_owned(),
    }
}

fn metric_id(name: &str) -> Result<QualityMetricId, QualityBuildError> {
    Ok(QualityMetricId::new(
        METRIC_NAMESPACE,
        name,
        METRIC_VERSION,
    )?)
}

fn poll_cancelled(
    cancellation: Option<&BuildCancellation>,
    index: usize,
) -> Result<(), QualityBuildError> {
    if index & CANCELLATION_POLL_MASK == 0 {
        if let Some(cancellation) = cancellation {
            if cancellation.is_cancelled() {
                return Err(QualityBuildError::Cancelled);
            }
        }
    }
    Ok(())
}

/// Rejects any report that is not the locked evaluator's exact verdict.
pub(crate) fn validate_surface_formation_quality_report(
    report: &NaturalQualityReport,
    expected_surface: SurfaceRef,
    expected_checkpoint_fingerprint: &[u8; 32],
    profile: NaturalQualityProfile,
) -> Result<(), String> {
    report.validate().map_err(|error| error.to_string())?;
    if report.surface_ref() != expected_surface {
        return Err("P5 quality report is not bound to formation authority".to_owned());
    }
    if report.subject_fingerprint() != Some(expected_checkpoint_fingerprint) {
        return Err("P5 quality report is not bound to the exact formation checkpoint".to_owned());
    }
    if report.metrics().len() != EXPECTED_METRIC_NAMES.len() {
        return Err(format!(
            "P5 quality report contains {} metrics; expected {}",
            report.metrics().len(),
            EXPECTED_METRIC_NAMES.len()
        ));
    }
    let bounds = expected_metric_bounds(profile);
    for ((metric, expected_name), (expected_min, expected_max)) in report
        .metrics()
        .iter()
        .zip(EXPECTED_METRIC_NAMES)
        .zip(bounds)
    {
        if metric.id().namespace() != METRIC_NAMESPACE
            || metric.id().version() != METRIC_VERSION
            || metric.id().name() != expected_name
        {
            return Err(format!("unexpected P5 metric {}", metric.id().name()));
        }
        if metric.bounds().min() != expected_min || metric.bounds().max() != expected_max {
            return Err(format!(
                "P5 metric {expected_name} changed locked bounds from {expected_min:?}..={expected_max:?} to {:?}..={:?}",
                metric.bounds().min(),
                metric.bounds().max()
            ));
        }
        // Per-world metric statuses are measurements of this world, not
        // gates: any recorded status is legal evidence and the runtime never
        // rejects a world for its statistics (user ruling, 2026-08-20).
        // Structural checks - binding, metric set, locked bounds, sample
        // counts - stay hard because failing them means the evidence itself
        // is corrupt.
        if metric.status() == QualityMetricStatus::Unavailable {
            continue;
        }
        if metric.sample_count() > expected_surface.cell_count() {
            return Err(format!(
                "P5 metric {expected_name} reports {} samples; maximum is {}",
                metric.sample_count(),
                expected_surface.cell_count()
            ));
        }
    }
    Ok(())
}
