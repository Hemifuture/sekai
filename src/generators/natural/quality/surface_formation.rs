//! Locked P5 conservation, causality, and formation-morphology quality gates.

use super::{MetricObservation, NaturalQualityReportBuilder, QualityBuildError};
use crate::engine::BuildCancellation;
use crate::world::natural::{
    formation_elevation_from_components, hypsometric_mean, hypsometric_quantile,
    hypsometric_share_below, sort_hypsometric_samples, LandOceanKind, NaturalQualityProfile,
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
const NO_OCEAN_REASON: &str = "the published world has no ocean";
const NO_NETWORK_LAND_REASON: &str = "dry land covers at most 10% of the published world";
const NO_DEPOSIT_REASON: &str = "the solve deposited no sediment anywhere";
const NO_INCISION_REASON: &str = "the solve produced no fluvial incision";

/// Ceiling above sea level of the reported coastal-lowland share.
const LOWLAND_CEILING_M: f32 = 100.0;
const NO_CORPUS_REASON: &str = "no world in the corpus reported this hypsometric measurement";
const CORPUS_METRIC_PREFIX: &str = "corpus-median-";

/// The per-world hypsometric measurements and the ETOPO1-anchored envelope
/// their 17-seed corpus medians must satisfy (T0 calibration spec §3.2).
/// Per-world reports carry the measurements unbounded: single-seed quantiles
/// scatter by tens of percent, so the envelope is a corpus gate exactly like
/// the P3 statistical gates.
const HYPSOMETRY_ENVELOPE: [(&str, Option<f64>, Option<f64>); 8] = [
    ("land-area-share-below-100m", Some(0.10), None),
    ("land-relief-mean-m", Some(600.0), Some(1_000.0)),
    ("land-relief-p05-m", Some(0.0), Some(80.0)),
    ("land-relief-p25-m", Some(80.0), Some(350.0)),
    ("land-relief-p50-m", Some(300.0), Some(700.0)),
    ("land-relief-p75-m", Some(700.0), Some(1_400.0)),
    ("land-relief-p95-m", Some(1_800.0), Some(3_400.0)),
    ("ocean-depth-p50-m", Some(2_800.0), Some(4_800.0)),
];

/// Every per-world metric name in report (alphabetical) order.
const EXPECTED_METRIC_NAMES: [&str; 22] = [
    "component-identity-mismatch-count",
    "deposited-sediment-enrichment-ratio",
    "equilibrium-current-flux-residual",
    "final-land-fraction-absolute-change",
    "fluvial-incision-support-enrichment-ratio",
    "land-area-share-below-100m",
    "land-outlet-path-area-fraction",
    "land-relief-mean-m",
    "land-relief-p05-m",
    "land-relief-p25-m",
    "land-relief-p50-m",
    "land-relief-p75-m",
    "land-relief-p95-m",
    "largest-network-strahler-order",
    "ocean-depth-p50-m",
    "primary-final-elevation-correlation",
    "provenance-mass-relative-error",
    "receiver-adjacency-violation-count",
    "river-reach-count",
    "sediment-mass-relative-error",
    "through-ocean-land-river-count",
    "water-volume-relative-error",
];

/// Returns the locked per-profile bounds in the canonical metric order; the
/// hypsometric measurements are unbounded per world (see
/// [`HYPSOMETRY_ENVELOPE`]).
fn expected_metric_bounds(profile: NaturalQualityProfile) -> [(Option<f64>, Option<f64>); 22] {
    let strahler_min = match profile {
        NaturalQualityProfile::Draft => 3.0,
        NaturalQualityProfile::Standard | NaturalQualityProfile::High => 4.0,
    };
    [
        (None, Some(0.0)),
        (Some(1.25), None),
        (None, Some(1.0)),
        (None, Some(0.03)),
        (Some(1.50), None),
        (None, None),
        (Some(0.95), None),
        (None, None),
        (None, None),
        (None, None),
        (None, None),
        (None, None),
        (None, None),
        (Some(strahler_min), None),
        (None, None),
        (Some(0.90), None),
        (None, Some(SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX)),
        (None, Some(0.0)),
        (Some(1.0), None),
        (None, Some(SEDIMENT_BUDGET_RELATIVE_ERROR_MAX)),
        (None, Some(0.0)),
        (None, Some(WATER_VOLUME_RELATIVE_TOLERANCE)),
    ]
}

/// Looks one locked per-world bound pair up by metric name.
fn locked_bounds(
    bounds: &[(Option<f64>, Option<f64>); 22],
    name: &str,
) -> (Option<f64>, Option<f64>) {
    let position = EXPECTED_METRIC_NAMES
        .iter()
        .position(|&expected| expected == name)
        .expect("locked metric names are exhaustive");
    bounds[position]
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
            .primary_relief_m()
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
    let maximum = |name: &str| locked_bounds(&bounds, name).1.expect("locked maximum");
    let minimum = |name: &str| locked_bounds(&bounds, name).0.expect("locked minimum");
    let cells = surface.cells().len() as u32;

    let name = "component-identity-mismatch-count";
    builder.record_at_most(
        metric_id(name)?,
        state.component_identity_mismatch_count,
        cells,
        maximum(name),
    )?;
    let name = "deposited-sediment-enrichment-ratio";
    builder.record_observation_at_least(
        metric_id(name)?,
        state.deposited_sediment_enrichment(),
        minimum(name),
    )?;
    let name = "equilibrium-current-flux-residual";
    builder.record_at_most(
        metric_id(name)?,
        snapshot.solve_report().terminal_residual().normalized_max(),
        u32::from(snapshot.solve_report().climate_solve_count()),
        maximum(name),
    )?;
    let name = "final-land-fraction-absolute-change";
    builder.record_at_most(
        metric_id(name)?,
        state.land_fraction_absolute_change,
        cells,
        maximum(name),
    )?;
    let name = "fluvial-incision-support-enrichment-ratio";
    builder.record_observation_at_least(
        metric_id(name)?,
        state.fluvial_incision_enrichment(),
        minimum(name),
    )?;
    let name = "land-outlet-path-area-fraction";
    builder.record_observation_at_least(
        metric_id(name)?,
        state.land_outlet_path_fraction(),
        minimum(name),
    )?;
    let name = "largest-network-strahler-order";
    builder.record_observation_at_least(
        metric_id(name)?,
        state.largest_network_strahler_order(),
        minimum(name),
    )?;
    let name = "primary-final-elevation-correlation";
    builder.record_observation_at_least(
        metric_id(name)?,
        available(state.primary_final_correlation, cells),
        minimum(name),
    )?;
    let name = "provenance-mass-relative-error";
    builder.record_at_most(
        metric_id(name)?,
        state.provenance_relative_error,
        cells,
        maximum(name),
    )?;
    let name = "receiver-adjacency-violation-count";
    builder.record_at_most(
        metric_id(name)?,
        state.receiver_adjacency_violation_count,
        cells,
        maximum(name),
    )?;
    let name = "river-reach-count";
    builder.record_observation_at_least(
        metric_id(name)?,
        state.river_reach_count(),
        minimum(name),
    )?;
    let name = "sediment-mass-relative-error";
    builder.record_at_most(
        metric_id(name)?,
        snapshot.sediment_budget_report().global_relative_error(),
        cells,
        maximum(name),
    )?;
    let name = "through-ocean-land-river-count";
    builder.record_at_most(
        metric_id(name)?,
        state.through_ocean_land_river_count,
        cells,
        maximum(name),
    )?;
    let name = "water-volume-relative-error";
    builder.record_at_most(
        metric_id(name)?,
        state.water_volume_relative_error,
        cells,
        maximum(name),
    )?;
    for (name, observation) in [
        ("land-area-share-below-100m", state.lowland_share()),
        ("land-relief-mean-m", state.land_relief_mean()),
        ("land-relief-p05-m", state.land_relief_quantile(0.05)),
        ("land-relief-p25-m", state.land_relief_quantile(0.25)),
        ("land-relief-p50-m", state.land_relief_quantile(0.50)),
        ("land-relief-p75-m", state.land_relief_quantile(0.75)),
        ("land-relief-p95-m", state.land_relief_quantile(0.95)),
        ("ocean-depth-p50-m", state.ocean_depth_median()),
    ] {
        builder.record_observation_unbounded(metric_id(name)?, observation)?;
    }

    Ok(builder
        .finish()?
        .bind_subject_fingerprint(*snapshot.checkpoint().fingerprint())?)
}

/// Gates the T0 hypsometric envelope on the corpus medians of the per-world
/// hypsometric measurements of every supplied report (the P3 corpus-gate
/// precedent); each corpus metric is `corpus-median-<name>` with the frozen
/// envelope bounds.
pub fn evaluate_surface_formation_corpus_hypsometry(
    reports: &[NaturalQualityReport],
) -> Result<NaturalQualityReport, QualityBuildError> {
    let Some(first) = reports.first() else {
        return Err(QualityBuildError::InvalidInput {
            input: "surface-formation-corpus",
            reason: "the hypsometry corpus is empty".to_owned(),
        });
    };
    if reports
        .iter()
        .any(|report| report.surface_ref() != first.surface_ref())
    {
        return Err(QualityBuildError::InvalidInput {
            input: "surface-formation-corpus",
            reason: "the hypsometry corpus mixes authoritative surfaces".to_owned(),
        });
    }
    let mut builder = NaturalQualityReportBuilder::new(first.surface_ref());
    for (name, min, max) in HYPSOMETRY_ENVELOPE {
        let mut values = reports
            .iter()
            .filter_map(|report| {
                report
                    .metrics()
                    .iter()
                    .find(|metric| metric.id().name() == name)
                    .and_then(|metric| metric.value())
            })
            .collect::<Vec<_>>();
        values.sort_by(f64::total_cmp);
        let observation = if values.is_empty() {
            unavailable(NO_CORPUS_REASON)
        } else {
            available(super::median_sorted_f64(&values), values.len() as u32)
        };
        let id = metric_id(&format!("{CORPUS_METRIC_PREFIX}{name}"))?;
        match (min, max) {
            (Some(min), Some(max)) => {
                builder.record_observation_between(id, observation, min, max)?
            }
            (Some(min), None) => builder.record_observation_at_least(id, observation, min)?,
            (None, Some(max)) => builder.record_observation_at_most(id, observation, max)?,
            (None, None) => builder.record_observation_unbounded(id, observation)?,
        }
    }
    builder.finish()
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
    /// Land relief above the solved sea level with cell area, sorted by value.
    land_relief_samples: Vec<(f32, f64)>,
    /// Ocean depth below the solved sea level with cell area, sorted by value.
    ocean_depth_samples: Vec<(f32, f64)>,
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
        let process_rates = snapshot.process_rates();
        let hydrology = snapshot.hydrology();
        let sediment = terrain.sediment();
        let elevation = terrain.current_elevation_m();
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
            land_relief_samples: Vec::new(),
            ocean_depth_samples: Vec::new(),
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
            primary_sum += area_m2 * f64::from(components.primary_relief_m()[index]);
            final_sum += area_m2 * f64::from(elevation[index]);

            let expected = formation_elevation_from_components(
                components.primary_relief_m()[index],
                components.equilibrium_adjustment_m()[index],
            );
            if elevation[index].to_bits() != expected.to_bits() {
                state.component_identity_mismatch_count += 1.0;
            }

            if relief.land_ocean().raw_values()[index] == LandOceanKind::Land.raw() {
                relief_land_area_m2 += area_m2;
            }
            let water = hydrology.surface_water().get(index);
            let is_land = terrain.land_ocean().raw_values()[index] == LandOceanKind::Land.raw();
            let relief_m = elevation[index] - terrain.sea_level_m();
            if is_land {
                state.land_relief_samples.push((relief_m, area_m2));
            } else {
                state.ocean_depth_samples.push((-relief_m, area_m2));
            }
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

            let incision_m3 =
                f64::from(process_rates.fluvial_erosion_rate_m_per_year()[index]) * area_m2;
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
            components.primary_relief_m(),
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
                        f64::from(process_rates.fluvial_erosion_rate_m_per_year()[index]) * area_m2;
                }
            }
        }
        sort_hypsometric_samples(&mut state.land_relief_samples);
        sort_hypsometric_samples(&mut state.ocean_depth_samples);
        Ok(state)
    }

    fn land_relief_quantile(&self, quantile: f64) -> MetricObservation {
        if self.land_relief_samples.is_empty() {
            return unavailable(NO_LAND_REASON);
        }
        available(
            f64::from(hypsometric_quantile(&self.land_relief_samples, quantile)),
            self.land_cell_count,
        )
    }

    fn land_relief_mean(&self) -> MetricObservation {
        if self.land_relief_samples.is_empty() {
            return unavailable(NO_LAND_REASON);
        }
        available(
            hypsometric_mean(&self.land_relief_samples),
            self.land_cell_count,
        )
    }

    fn lowland_share(&self) -> MetricObservation {
        if self.land_relief_samples.is_empty() {
            return unavailable(NO_LAND_REASON);
        }
        available(
            hypsometric_share_below(&self.land_relief_samples, LOWLAND_CEILING_M),
            self.land_cell_count,
        )
    }

    fn ocean_depth_median(&self) -> MetricObservation {
        if self.ocean_depth_samples.is_empty() {
            return unavailable(NO_OCEAN_REASON);
        }
        available(
            f64::from(hypsometric_quantile(&self.ocean_depth_samples, 0.5)),
            self.ocean_depth_samples.len() as u32,
        )
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
