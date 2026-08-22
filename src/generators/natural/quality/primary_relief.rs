//! Versioned scientific and morphology evidence for physical P3 relief.

use super::{MetricObservation, NaturalQualityReportBuilder, QualityBuildError};
use crate::generators::natural::evaluate_evolved_tectonic_quality;
use crate::world::natural::{
    scaled_earth_ocean_inventory_m3, BoundaryKind, CrustKind, EvolvedTectonicSnapshot,
    GeologicSubstrateSnapshot, NaturalQualityReport, PrimaryReliefSnapshot, QualityMetricId,
    QualityMetricStatus, ELEVATION_MAX_M, ELEVATION_MIN_M,
};
use crate::world::spatial::SphericalSurfaceSnapshot;

const METRIC_NAMESPACE: &str = "sekai.primary-relief-v1";
const METRIC_VERSION: u16 = 1;
const EXPECTED_METRIC_NAMES: [&str; 15] = [
    "coast-plate-boundary-overlap",
    "component-closure-max-error-m",
    "continental-ocean-median-separation-m",
    "convergent-positive-dynamic-fraction",
    "elevation-safety-violation-count",
    "hotspot-positive-construction-fraction",
    "maximum-plate-area-fraction",
    "non-finite-value-count",
    "old-young-ocean-depth-separation-m",
    "physical-land-area-fraction",
    "regional-detail-rms-ratio",
    "subduction-negative-dynamic-fraction",
    "upstream-p2-hard-failure-count",
    "water-inventory-ratio",
    "water-volume-relative-error",
];
const P2_CORPUS_SCOPED_NAMES: [&str; 6] = [
    "collision-causality-fraction",
    "continental-area-fraction",
    "ocean-age-depth-spearman",
    "regular-triple-junction-angle-fraction",
    "subduction-causality-fraction",
    "transform-to-convergent-uplift-ratio",
];

/// One raw P3 corpus member; quality is always recomputed from these snapshots.
#[derive(Debug, Clone, Copy)]
pub struct PrimaryReliefQualitySample<'a> {
    evolved: &'a EvolvedTectonicSnapshot,
    substrate: &'a GeologicSubstrateSnapshot,
    relief: &'a PrimaryReliefSnapshot,
}

impl<'a> PrimaryReliefQualitySample<'a> {
    pub const fn new(
        evolved: &'a EvolvedTectonicSnapshot,
        substrate: &'a GeologicSubstrateSnapshot,
        relief: &'a PrimaryReliefSnapshot,
    ) -> Self {
        Self {
            evolved,
            substrate,
            relief,
        }
    }
}

/// Evaluates hard and statistical P3 gates for one authoritative world.
pub fn evaluate_primary_relief_quality(
    surface: &SphericalSurfaceSnapshot,
    evolved: &EvolvedTectonicSnapshot,
    substrate: &GeologicSubstrateSnapshot,
    relief: &PrimaryReliefSnapshot,
) -> Result<NaturalQualityReport, QualityBuildError> {
    validate_inputs(surface, evolved, substrate, relief)?;
    let raw = RawReliefMetrics::collect(surface, evolved, substrate, relief)?;
    let p2 = evaluate_evolved_tectonic_quality(surface, evolved)?;
    let maximum_plate_area = metric_value(&p2, "maximum-plate-area-fraction")?;
    let upstream_hard_failures = p2
        .metrics()
        .iter()
        .filter(|metric| !P2_CORPUS_SCOPED_NAMES.contains(&metric.id().name()))
        .filter(|metric| metric.status() != QualityMetricStatus::Pass)
        .count();
    let (non_finite, inspected) = non_finite_count(substrate, relief);
    let (component_error, component_samples) = component_closure(relief);
    let elevation_violations = relief
        .elevation_m()
        .iter()
        .filter(|&&value| !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&value))
        .count();

    let mut builder = NaturalQualityReportBuilder::new(relief.surface_ref());
    record_statistical_metrics(&mut builder, &raw)?;
    builder.record_at_most(
        metric_id("component-closure-max-error-m")?,
        component_error,
        count(component_samples, "component-closure")?,
        0.02,
    )?;
    builder.record_at_most(
        metric_id("elevation-safety-violation-count")?,
        elevation_violations as f64,
        count(relief.elevation_m().len(), "elevation-safety")?,
        0.0,
    )?;
    builder.record_at_most(
        metric_id("maximum-plate-area-fraction")?,
        maximum_plate_area,
        count(evolved.compatibility().plates().len(), "plates")?,
        0.45,
    )?;
    builder.record_at_most(
        metric_id("non-finite-value-count")?,
        non_finite as f64,
        count(inspected, "non-finite-values")?,
        0.0,
    )?;
    builder.record_at_most(
        metric_id("upstream-p2-hard-failure-count")?,
        upstream_hard_failures as f64,
        count(p2.metrics().len(), "upstream-p2-metrics")?,
        0.0,
    )?;
    builder.record_at_most(
        metric_id("water-volume-relative-error")?,
        relief.water_volume_relative_error(),
        count(relief.elevation_m().len(), "water-volume")?,
        1.0e-6,
    )?;
    let report = builder.finish()?;
    validate_primary_relief_quality_report(&report, relief.surface_ref())
        .map_err(|reason| invalid_input("primary-relief-quality-report", reason))?;
    Ok(report)
}

/// Evaluates the statistical gates and measurements over original fixed-seed samples.
pub fn evaluate_primary_relief_corpus_quality(
    surface: &SphericalSurfaceSnapshot,
    samples: &[PrimaryReliefQualitySample<'_>],
) -> Result<NaturalQualityReport, QualityBuildError> {
    if samples.is_empty() {
        return Err(invalid_input(
            "primary-relief-corpus",
            "the quality corpus is empty".to_owned(),
        ));
    }
    let mut corpus = RawReliefMetrics::default();
    for sample in samples {
        validate_inputs(surface, sample.evolved, sample.substrate, sample.relief)?;
        corpus.extend(RawReliefMetrics::collect(
            surface,
            sample.evolved,
            sample.substrate,
            sample.relief,
        )?)?;
    }
    let mut builder = NaturalQualityReportBuilder::new(samples[0].relief.surface_ref());
    record_statistical_metrics(&mut builder, &corpus)?;
    builder.finish()
}

fn record_statistical_metrics(
    builder: &mut NaturalQualityReportBuilder,
    raw: &RawReliefMetrics,
) -> Result<(), QualityBuildError> {
    builder.record_observation_at_most(
        metric_id("coast-plate-boundary-overlap")?,
        median_observation(
            &raw.coast_overlaps,
            "no classified coastline in the quality sample",
            "coast-overlap",
        )?,
        0.35,
    )?;
    builder.record_observation_at_least(
        metric_id("continental-ocean-median-separation-m")?,
        separation_observation(&raw.continental_elevation, &raw.ocean_elevation)?,
        2_500.0,
    )?;
    builder.record_observation_at_least(
        metric_id("convergent-positive-dynamic-fraction")?,
        raw.convergent.finish(
            "no active convergent uplift cells in the quality sample",
            "convergent-dynamic",
        )?,
        0.80,
    )?;
    builder.record_observation_at_least(
        metric_id("hotspot-positive-construction-fraction")?,
        raw.hotspots.finish(
            "no mantle hotspots in the quality sample",
            "hotspot-construction",
        )?,
        0.80,
    )?;
    builder.record_observation_at_least(
        metric_id("old-young-ocean-depth-separation-m")?,
        separation_observation(&raw.old_ocean_depth, &raw.young_ocean_depth)?,
        600.0,
    )?;
    builder.record_observation_between(
        metric_id("physical-land-area-fraction")?,
        median_observation(
            &raw.physical_land_fractions,
            "no physical land-fraction samples",
            "physical-land-fraction",
        )?,
        0.20,
        0.55,
    )?;
    builder.record_observation_between(
        metric_id("regional-detail-rms-ratio")?,
        raw.rms_ratio()?,
        0.01,
        0.30,
    )?;
    builder.record_observation_at_least(
        metric_id("subduction-negative-dynamic-fraction")?,
        raw.subduction.finish(
            "no descending subduction cells in the quality sample",
            "subduction-dynamic",
        )?,
        0.80,
    )?;
    builder.record_observation_unbounded(
        metric_id("water-inventory-ratio")?,
        median_observation(
            &raw.water_inventory_ratios,
            "no water-inventory samples",
            "water-inventory-ratio",
        )?,
    )?;
    Ok(())
}

#[derive(Debug, Default)]
struct RawReliefMetrics {
    continental_elevation: Vec<f32>,
    ocean_elevation: Vec<f32>,
    convergent: FractionAggregate,
    subduction: FractionAggregate,
    young_ocean_depth: Vec<f32>,
    old_ocean_depth: Vec<f32>,
    detail_weighted_square: f64,
    elevation_weighted_square: f64,
    rms_samples: u64,
    hotspots: FractionAggregate,
    coast_overlaps: Vec<f64>,
    physical_land_fractions: Vec<f64>,
    water_inventory_ratios: Vec<f64>,
}

impl RawReliefMetrics {
    fn collect(
        surface: &SphericalSurfaceSnapshot,
        evolved: &EvolvedTectonicSnapshot,
        substrate: &GeologicSubstrateSnapshot,
        relief: &PrimaryReliefSnapshot,
    ) -> Result<Self, QualityBuildError> {
        let mut raw = Self::default();
        for (index, cell) in surface.cells().iter().enumerate() {
            let elevation = relief.elevation_m()[index];
            match substrate.crust_kind(index) {
                Some(CrustKind::Continental) => raw.continental_elevation.push(elevation),
                Some(CrustKind::Oceanic) => {
                    raw.ocean_elevation.push(elevation);
                    let age = substrate.ocean_age_myr()[index];
                    if age <= 20.0 {
                        raw.young_ocean_depth.push(-elevation);
                    }
                    if age >= 80.0 {
                        raw.old_ocean_depth.push(-elevation);
                    }
                }
                None => {
                    return Err(invalid_input(
                        "substrate-crust",
                        format!("missing crust kind at cell {index}"),
                    ));
                }
            }
            let area = cell.area.get();
            raw.detail_weighted_square +=
                area * f64::from(relief.conditioned_regional_detail_m()[index]).powi(2);
            raw.elevation_weighted_square += area * f64::from(elevation).powi(2);
            raw.rms_samples += 1;
        }
        append_boundary_samples(surface, evolved, relief, &mut raw)?;
        for hotspot in substrate.mantle().hotspots() {
            raw.hotspots.push(
                relief.volcanic_construction_m()[hotspot.source_cell().raw() as usize] > 0.0,
            )?;
        }
        if let Some(overlap) = coast_plate_overlap(surface, evolved, relief) {
            raw.coast_overlaps.push(overlap);
        }
        raw.physical_land_fractions
            .push(f64::from(relief.physical_land_fraction()));
        let earth_inventory = scaled_earth_ocean_inventory_m3(surface.total_cell_area().get())
            .map_err(|error| invalid_input("water-inventory", error.to_string()))?;
        raw.water_inventory_ratios
            .push(relief.water_inventory_m3() / earth_inventory);
        Ok(raw)
    }

    fn extend(&mut self, other: Self) -> Result<(), QualityBuildError> {
        self.continental_elevation
            .extend(other.continental_elevation);
        self.ocean_elevation.extend(other.ocean_elevation);
        self.convergent.extend(other.convergent)?;
        self.subduction.extend(other.subduction)?;
        self.young_ocean_depth.extend(other.young_ocean_depth);
        self.old_ocean_depth.extend(other.old_ocean_depth);
        self.detail_weighted_square += other.detail_weighted_square;
        self.elevation_weighted_square += other.elevation_weighted_square;
        if !self.detail_weighted_square.is_finite() || !self.elevation_weighted_square.is_finite() {
            return Err(QualityBuildError::NonFiniteAccumulation);
        }
        self.rms_samples = self
            .rms_samples
            .checked_add(other.rms_samples)
            .ok_or(QualityBuildError::SampleCountOverflow)?;
        self.hotspots.extend(other.hotspots)?;
        self.coast_overlaps.extend(other.coast_overlaps);
        self.physical_land_fractions
            .extend(other.physical_land_fractions);
        self.water_inventory_ratios
            .extend(other.water_inventory_ratios);
        Ok(())
    }

    fn rms_ratio(&self) -> Result<MetricObservation, QualityBuildError> {
        if self.rms_samples == 0 {
            return Ok(MetricObservation::Unavailable {
                reason: "no cell RMS samples".to_owned(),
            });
        }
        if self.elevation_weighted_square <= 0.0 {
            return Ok(MetricObservation::Unavailable {
                reason: "total elevation RMS is zero".to_owned(),
            });
        }
        let value = (self.detail_weighted_square / self.elevation_weighted_square).sqrt();
        if !value.is_finite() {
            return Err(QualityBuildError::NonFiniteAccumulation);
        }
        Ok(MetricObservation::Available {
            value,
            sample_count: u32::try_from(self.rms_samples)
                .map_err(|_| QualityBuildError::SampleCountOverflow)?,
        })
    }
}

fn append_boundary_samples(
    surface: &SphericalSurfaceSnapshot,
    evolved: &EvolvedTectonicSnapshot,
    relief: &PrimaryReliefSnapshot,
    raw: &mut RawReliefMetrics,
) -> Result<(), QualityBuildError> {
    let tectonic = evolved.compatibility();
    let forcing = evolved.forcing();
    for (edge, boundary) in surface.edges().iter().zip(tectonic.boundaries()) {
        let [first, second] = edge.cells.map(|cell| cell.raw() as usize);
        match boundary.kind {
            BoundaryKind::Subduction => {
                let descending = boundary.subducting_plate.ok_or_else(|| {
                    invalid_input("subduction", "missing descending plate".to_owned())
                })?;
                let first_owner = tectonic
                    .plate_for_cell(edge.cells[0])
                    .ok_or_else(|| invalid_input("cell-plates", format!("missing cell {first}")))?;
                let (descending_cell, overriding_cell) = if first_owner == descending {
                    (first, second)
                } else {
                    (second, first)
                };
                if forcing.subsidence_rate_mm_per_year()[descending_cell]
                    > forcing.uplift_rate_mm_per_year()[descending_cell]
                {
                    raw.subduction
                        .push(relief.dynamic_tectonic_offset_m()[descending_cell] < 0.0)?;
                }
                if forcing.uplift_rate_mm_per_year()[overriding_cell]
                    > forcing.subsidence_rate_mm_per_year()[overriding_cell]
                {
                    raw.convergent
                        .push(relief.dynamic_tectonic_offset_m()[overriding_cell] > 0.0)?;
                }
            }
            BoundaryKind::ContinentalCollision => {
                for index in [first, second] {
                    if forcing.uplift_rate_mm_per_year()[index]
                        > forcing.subsidence_rate_mm_per_year()[index]
                    {
                        raw.convergent
                            .push(relief.dynamic_tectonic_offset_m()[index] > 0.0)?;
                    }
                }
            }
            BoundaryKind::None
            | BoundaryKind::Weak
            | BoundaryKind::ContinentalRift
            | BoundaryKind::OceanicRidge
            | BoundaryKind::Transform => {}
        }
    }
    Ok(())
}

fn coast_plate_overlap(
    surface: &SphericalSurfaceSnapshot,
    evolved: &EvolvedTectonicSnapshot,
    relief: &PrimaryReliefSnapshot,
) -> Option<f64> {
    let tectonic = evolved.compatibility();
    let mut boundary_cells = vec![false; surface.cells().len()];
    for edge in surface.edges() {
        if tectonic.plate_for_cell(edge.cells[0]) != tectonic.plate_for_cell(edge.cells[1]) {
            boundary_cells[edge.cells[0].raw() as usize] = true;
            boundary_cells[edge.cells[1].raw() as usize] = true;
        }
    }
    let mut coast_length = 0.0;
    let mut overlap_length = 0.0;
    for edge in surface.edges() {
        if relief.land_ocean().get(edge.cells[0].raw() as usize)
            == relief.land_ocean().get(edge.cells[1].raw() as usize)
        {
            continue;
        }
        coast_length += edge.length.get();
        if edge
            .cells
            .iter()
            .any(|cell| boundary_cells[cell.raw() as usize])
        {
            overlap_length += edge.length.get();
        }
    }
    (coast_length > 0.0).then_some(overlap_length / coast_length)
}

fn component_closure(relief: &PrimaryReliefSnapshot) -> (f64, usize) {
    let maximum = (0..relief.elevation_m().len())
        .map(|index| {
            let calculated = relief.isostatic_base_m()[index]
                + relief.dynamic_tectonic_offset_m()[index]
                + relief.volcanic_construction_m()[index]
                + relief.passive_margin_offset_m()[index]
                + relief.conditioned_regional_detail_m()[index];
            f64::from((relief.elevation_m()[index] - calculated).abs())
        })
        .fold(0.0_f64, f64::max);
    (maximum, relief.elevation_m().len())
}

fn non_finite_count(
    substrate: &GeologicSubstrateSnapshot,
    relief: &PrimaryReliefSnapshot,
) -> (usize, usize) {
    let fields = [
        substrate.crust_density_kg_m3(),
        substrate.fracture_intensity(),
        substrate.erodibility(),
        substrate.relative_permeability(),
        substrate.heat_flow_mw_m2(),
        substrate.volcanic_influence(),
        relief.isostatic_base_m(),
        relief.dynamic_tectonic_offset_m(),
        relief.volcanic_construction_m(),
        relief.passive_margin_offset_m(),
        relief.conditioned_regional_detail_m(),
        relief.elevation_m(),
    ];
    let mut inspected = fields.iter().map(|field| field.len()).sum::<usize>();
    let mut non_finite = fields
        .iter()
        .flat_map(|field| field.iter())
        .filter(|value| !value.is_finite())
        .count();
    let scalars = [
        relief.water_inventory_m3(),
        relief.realized_water_volume_m3(),
        relief.water_volume_relative_error(),
    ];
    inspected += scalars.len();
    non_finite += scalars.iter().filter(|value| !value.is_finite()).count();
    (non_finite, inspected)
}

fn validate_inputs(
    surface: &SphericalSurfaceSnapshot,
    evolved: &EvolvedTectonicSnapshot,
    substrate: &GeologicSubstrateSnapshot,
    relief: &PrimaryReliefSnapshot,
) -> Result<(), QualityBuildError> {
    surface
        .validate()
        .map_err(|error| invalid_input("surface", error.to_string()))?;
    evolved
        .validate_against(surface)
        .map_err(|error| invalid_input("evolved-tectonics", error.to_string()))?;
    substrate
        .validate_against(surface, evolved)
        .map_err(|error| invalid_input("geologic-substrate", error.to_string()))?;
    relief
        .validate_against_surface_measurements(surface)
        .map_err(|error| invalid_input("primary-relief", error.to_string()))?;
    Ok(())
}

#[derive(Debug, Default)]
struct FractionAggregate {
    passed: u64,
    total: u64,
}

impl FractionAggregate {
    fn push(&mut self, passed: bool) -> Result<(), QualityBuildError> {
        self.total = self
            .total
            .checked_add(1)
            .ok_or(QualityBuildError::SampleCountOverflow)?;
        self.passed = self
            .passed
            .checked_add(u64::from(passed))
            .ok_or(QualityBuildError::SampleCountOverflow)?;
        Ok(())
    }

    fn extend(&mut self, other: Self) -> Result<(), QualityBuildError> {
        self.passed = self
            .passed
            .checked_add(other.passed)
            .ok_or(QualityBuildError::SampleCountOverflow)?;
        self.total = self
            .total
            .checked_add(other.total)
            .ok_or(QualityBuildError::SampleCountOverflow)?;
        Ok(())
    }

    fn finish(
        &self,
        empty_reason: &'static str,
        field: &'static str,
    ) -> Result<MetricObservation, QualityBuildError> {
        if self.total == 0 {
            return Ok(MetricObservation::Unavailable {
                reason: empty_reason.to_owned(),
            });
        }
        Ok(MetricObservation::Available {
            value: self.passed as f64 / self.total as f64,
            sample_count: u32::try_from(self.total).map_err(|_| {
                QualityBuildError::CountOverflow {
                    field,
                    found: usize::try_from(self.total).unwrap_or(usize::MAX),
                }
            })?,
        })
    }
}

fn separation_observation(
    higher: &[f32],
    lower: &[f32],
) -> Result<MetricObservation, QualityBuildError> {
    if higher.is_empty() || lower.is_empty() {
        return Ok(MetricObservation::Unavailable {
            reason: "both comparison populations require at least one sample".to_owned(),
        });
    }
    let value = median_f32(higher) - median_f32(lower);
    if !value.is_finite() {
        return Err(QualityBuildError::NonFiniteAccumulation);
    }
    Ok(MetricObservation::Available {
        value,
        sample_count: count(higher.len() + lower.len(), "population-separation")?,
    })
}

fn median_observation(
    values: &[f64],
    empty_reason: &'static str,
    field: &'static str,
) -> Result<MetricObservation, QualityBuildError> {
    if values.is_empty() {
        return Ok(MetricObservation::Unavailable {
            reason: empty_reason.to_owned(),
        });
    }
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    Ok(MetricObservation::Available {
        value: super::median_sorted_f64(&ordered),
        sample_count: count(ordered.len(), field)?,
    })
}

fn median_f32(values: &[f32]) -> f64 {
    let mut ordered = values.to_vec();
    ordered.sort_by(f32::total_cmp);
    let middle = ordered.len() / 2;
    if ordered.len() % 2 == 0 {
        (f64::from(ordered[middle - 1]) + f64::from(ordered[middle])) * 0.5
    } else {
        f64::from(ordered[middle])
    }
}

fn metric_value(
    report: &NaturalQualityReport,
    name: &'static str,
) -> Result<f64, QualityBuildError> {
    report
        .metrics()
        .iter()
        .find(|metric| metric.id().name() == name)
        .and_then(|metric| metric.value())
        .ok_or_else(|| invalid_input("upstream-p2-quality", format!("missing metric {name}")))
}

fn metric_id(name: &str) -> Result<QualityMetricId, QualityBuildError> {
    Ok(QualityMetricId::new(
        METRIC_NAMESPACE,
        name,
        METRIC_VERSION,
    )?)
}

fn count(found: usize, field: &'static str) -> Result<u32, QualityBuildError> {
    u32::try_from(found).map_err(|_| QualityBuildError::CountOverflow { field, found })
}

fn invalid_input(input: &'static str, reason: String) -> QualityBuildError {
    QualityBuildError::InvalidInput { input, reason }
}

/// Enforces exact P3 metric identity and every per-world hard gate.
pub(crate) fn validate_primary_relief_quality_report(
    report: &NaturalQualityReport,
    expected_surface: crate::world::spatial::SurfaceRef,
) -> Result<(), String> {
    report.validate().map_err(|error| error.to_string())?;
    if report.surface_ref() != expected_surface {
        return Err("P3 quality report is not bound to primary relief authority".to_owned());
    }
    if report.metrics().len() != EXPECTED_METRIC_NAMES.len() {
        return Err(format!(
            "P3 quality report contains {} metrics; expected {}",
            report.metrics().len(),
            EXPECTED_METRIC_NAMES.len()
        ));
    }
    for (metric, expected_name) in report.metrics().iter().zip(EXPECTED_METRIC_NAMES) {
        if metric.id().namespace() != METRIC_NAMESPACE
            || metric.id().version() != METRIC_VERSION
            || metric.id().name() != expected_name
        {
            return Err(format!("unexpected P3 metric {}", metric.id().name()));
        }
        // Per-world metric statuses are measurements of this world, not
        // gates: any recorded status is legal evidence and the runtime never
        // rejects a world for its statistics (user ruling, 2026-08-20).
        // Structural checks - binding, metric set, locked bounds, sample
        // counts - stay hard because failing them means the evidence itself
        // is corrupt.
    }
    Ok(())
}
