mod evolved_tectonics;
mod global_circulation;
mod primary_relief;
mod spatial;
mod spherical;

use thiserror::Error;

use crate::world::natural::{
    NaturalQualityReport, NaturalQualityValidationError, QualityBounds, QualityMetric,
    QualityMetricId, QualityMetricStatus, NATURAL_QUALITY_REPORT_SCHEMA_V1,
};
use crate::world::spatial::SurfaceRef;

pub(crate) use evolved_tectonics::validate_evolved_tectonic_quality_report;
pub use evolved_tectonics::{
    evaluate_evolved_tectonic_corpus_quality, evaluate_evolved_tectonic_quality,
};
pub use global_circulation::evaluate_global_circulation_quality;
pub(crate) use global_circulation::validate_global_circulation_quality_report;
pub(crate) use primary_relief::validate_primary_relief_quality_report;
pub use primary_relief::{
    evaluate_primary_relief_corpus_quality, evaluate_primary_relief_quality,
    PrimaryReliefQualitySample,
};
pub use spatial::evaluate_profile_surface_quality;
pub use spherical::evaluate_spherical_foundation_quality;
pub(crate) use spherical::{
    evaluate_spherical_foundation_quality_from_validated,
    validate_spherical_quality_input_identities,
};

const NO_POSITIVE_WEIGHT_REASON: &str = "no positive finite sample weight";

/// Builds a canonical report without allowing generator code to bypass metric validation.
pub(crate) struct NaturalQualityReportBuilder {
    surface_ref: SurfaceRef,
    metrics: Vec<QualityMetric>,
}

impl NaturalQualityReportBuilder {
    pub(crate) fn new(surface_ref: SurfaceRef) -> Self {
        Self {
            surface_ref,
            metrics: Vec::new(),
        }
    }

    pub(crate) fn record_at_most(
        &mut self,
        id: QualityMetricId,
        value: f64,
        sample_count: u32,
        max: f64,
    ) -> Result<(), QualityBuildError> {
        let bounds = QualityBounds::at_most(max)?;
        self.record_available(id, value, sample_count, bounds, value <= max)
    }

    pub(crate) fn record_between(
        &mut self,
        id: QualityMetricId,
        value: f64,
        sample_count: u32,
        min: f64,
        max: f64,
    ) -> Result<(), QualityBuildError> {
        let bounds = QualityBounds::between(min, max)?;
        self.record_available(
            id,
            value,
            sample_count,
            bounds,
            (min..=max).contains(&value),
        )
    }

    pub(crate) fn record_unbounded(
        &mut self,
        id: QualityMetricId,
        value: f64,
        sample_count: u32,
    ) -> Result<(), QualityBuildError> {
        self.record_available(id, value, sample_count, QualityBounds::unbounded(), true)
    }

    pub(crate) fn record_observation_at_least(
        &mut self,
        id: QualityMetricId,
        observation: MetricObservation,
        min: f64,
    ) -> Result<(), QualityBuildError> {
        let bounds = QualityBounds::at_least(min)?;
        self.record_observation(id, observation, bounds)
    }

    pub(crate) fn record_observation_at_most(
        &mut self,
        id: QualityMetricId,
        observation: MetricObservation,
        max: f64,
    ) -> Result<(), QualityBuildError> {
        let bounds = QualityBounds::at_most(max)?;
        self.record_observation(id, observation, bounds)
    }

    pub(crate) fn record_observation_between(
        &mut self,
        id: QualityMetricId,
        observation: MetricObservation,
        min: f64,
        max: f64,
    ) -> Result<(), QualityBuildError> {
        let bounds = QualityBounds::between(min, max)?;
        self.record_observation(id, observation, bounds)
    }

    fn record_available(
        &mut self,
        id: QualityMetricId,
        value: f64,
        sample_count: u32,
        bounds: QualityBounds,
        passes: bool,
    ) -> Result<(), QualityBuildError> {
        let status = if passes {
            QualityMetricStatus::Pass
        } else {
            QualityMetricStatus::Fail
        };
        self.metrics.push(QualityMetric::new(
            id,
            status,
            Some(value),
            sample_count,
            bounds,
            None,
        )?);
        Ok(())
    }

    fn record_observation(
        &mut self,
        id: QualityMetricId,
        observation: MetricObservation,
        bounds: QualityBounds,
    ) -> Result<(), QualityBuildError> {
        match observation {
            MetricObservation::Available {
                value,
                sample_count,
            } => self.record_available(
                id,
                value,
                sample_count,
                bounds,
                bounds_satisfied(bounds, value),
            ),
            MetricObservation::Unavailable { reason } => {
                self.metrics.push(QualityMetric::new(
                    id,
                    QualityMetricStatus::Unavailable,
                    None,
                    0,
                    bounds,
                    Some(reason),
                )?);
                Ok(())
            }
        }
    }

    pub(crate) fn finish(self) -> Result<NaturalQualityReport, QualityBuildError> {
        Ok(NaturalQualityReport::new(
            NATURAL_QUALITY_REPORT_SCHEMA_V1,
            self.surface_ref,
            self.metrics,
        )?)
    }
}

fn bounds_satisfied(bounds: QualityBounds, value: f64) -> bool {
    bounds.min().is_none_or(|min| value >= min) && bounds.max().is_none_or(|max| value <= max)
}

/// A finite weighted observation, or an explicit explanation for missing evidence.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MetricObservation {
    Available { value: f64, sample_count: u32 },
    Unavailable { reason: String },
}

#[cfg(test)]
impl MetricObservation {
    pub(crate) const fn value(&self) -> Option<f64> {
        match self {
            Self::Available { value, .. } => Some(*value),
            Self::Unavailable { .. } => None,
        }
    }

    pub(crate) const fn sample_count(&self) -> u32 {
        match self {
            Self::Available { sample_count, .. } => *sample_count,
            Self::Unavailable { .. } => 0,
        }
    }

    pub(crate) fn reason(&self) -> Option<&str> {
        match self {
            Self::Available { .. } => None,
            Self::Unavailable { reason } => Some(reason),
        }
    }
}

/// Accumulates a weighted mean with deterministic Neumaier compensation.
#[derive(Debug, Clone, Default)]
pub(crate) struct MetricAccumulator {
    numerator: NeumaierSum,
    denominator: NeumaierSum,
    sample_count: u32,
}

impl MetricAccumulator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, value: f64, weight: f64) -> Result<(), QualityBuildError> {
        if !value.is_finite() {
            return Err(QualityBuildError::NonFiniteSampleValue);
        }
        if !weight.is_finite() {
            return Err(QualityBuildError::NonFiniteSampleWeight);
        }
        if weight < 0.0 {
            return Err(QualityBuildError::NegativeSampleWeight { found: weight });
        }
        if weight == 0.0 {
            return Ok(());
        }
        let weighted_value = value * weight;
        if !weighted_value.is_finite() {
            return Err(QualityBuildError::NonFiniteWeightedValue);
        }
        self.sample_count = self
            .sample_count
            .checked_add(1)
            .ok_or(QualityBuildError::SampleCountOverflow)?;
        self.numerator.add(weighted_value)?;
        self.denominator.add(weight)?;
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<MetricObservation, QualityBuildError> {
        let denominator = self.denominator.total()?;
        if denominator == 0.0 {
            return Ok(MetricObservation::Unavailable {
                reason: NO_POSITIVE_WEIGHT_REASON.to_owned(),
            });
        }
        let value = self.numerator.total()? / denominator;
        if !value.is_finite() {
            return Err(QualityBuildError::NonFiniteAccumulation);
        }
        Ok(MetricObservation::Available {
            value,
            sample_count: self.sample_count,
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct NeumaierSum {
    sum: f64,
    correction: f64,
}

impl NeumaierSum {
    fn add(&mut self, value: f64) -> Result<(), QualityBuildError> {
        let next = self.sum + value;
        if !next.is_finite() {
            return Err(QualityBuildError::NonFiniteAccumulation);
        }
        let correction = if self.sum.abs() >= value.abs() {
            (self.sum - next) + value
        } else {
            (value - next) + self.sum
        };
        self.sum = next;
        self.correction += correction;
        if !self.correction.is_finite() {
            return Err(QualityBuildError::NonFiniteAccumulation);
        }
        Ok(())
    }

    fn total(self) -> Result<f64, QualityBuildError> {
        let total = self.sum + self.correction;
        if !total.is_finite() {
            return Err(QualityBuildError::NonFiniteAccumulation);
        }
        Ok(total)
    }
}

pub(crate) fn area_weighted_fraction(
    included: &[bool],
    weights: &[f64],
) -> Result<MetricObservation, QualityBuildError> {
    validate_equal_lengths(&[("included", included.len()), ("weights", weights.len())])?;
    let mut accumulator = MetricAccumulator::new();
    for (&included, &weight) in included.iter().zip(weights) {
        accumulator.push(if included { 1.0 } else { 0.0 }, weight)?;
    }
    accumulator.finish()
}

pub(crate) fn jaccard_fraction(
    left: &[bool],
    right: &[bool],
    weights: &[f64],
) -> Result<MetricObservation, QualityBuildError> {
    validate_equal_lengths(&[
        ("left", left.len()),
        ("right", right.len()),
        ("weights", weights.len()),
    ])?;
    let mut accumulator = MetricAccumulator::new();
    for ((&left, &right), &weight) in left.iter().zip(right).zip(weights) {
        let union_weight = if left || right { weight } else { 0.0 };
        accumulator.push(if left && right { 1.0 } else { 0.0 }, union_weight)?;
    }
    accumulator.finish()
}

fn validate_equal_lengths(lengths: &[(&'static str, usize)]) -> Result<(), QualityBuildError> {
    let expected = lengths.first().map_or(0, |(_, length)| *length);
    if let Some(&(field, found)) = lengths.iter().find(|(_, length)| *length != expected) {
        return Err(QualityBuildError::LengthMismatch {
            field,
            found,
            expected,
        });
    }
    Ok(())
}

/// Errors returned while deterministically assembling natural-quality evidence.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum QualityBuildError {
    #[error("invalid quality metric or report: {0}")]
    InvalidReport(#[from] NaturalQualityValidationError),
    #[error("quality sample values must be finite")]
    NonFiniteSampleValue,
    #[error("quality sample weights must be finite")]
    NonFiniteSampleWeight,
    #[error("quality sample weight {found} must be nonnegative")]
    NegativeSampleWeight { found: f64 },
    #[error("a weighted quality sample overflowed to a non-finite value")]
    NonFiniteWeightedValue,
    #[error("quality metric sample count exceeds u32::MAX")]
    SampleCountOverflow,
    #[error("quality metric accumulation produced a non-finite value")]
    NonFiniteAccumulation,
    #[error("quality input {field} has length {found}; expected {expected}")]
    LengthMismatch {
        field: &'static str,
        found: usize,
        expected: usize,
    },
    #[error("invalid spherical quality input {input}: {reason}")]
    InvalidInput { input: &'static str, reason: String },
    #[error("P1 conservative-remap fixture failed: {0}")]
    ConservativeRemap(#[from] crate::generators::spatial::ConservativeRemapError),
    #[error("remapped tangent vector at cell {cell:?} has radial residual {found} > {max}")]
    TangentResidualExceeded {
        cell: crate::world::CellId,
        found: f64,
        max: f64,
    },
    #[error("spherical quality input {input} references {found:?}; expected {expected:?}")]
    SurfaceMismatch {
        input: &'static str,
        found: SurfaceRef,
        expected: SurfaceRef,
    },
    #[error("quality input {field} count {found} exceeds the supported report range")]
    CountOverflow { field: &'static str, found: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::natural::{
        QualityMetricId, QualityMetricStatus, NATURAL_QUALITY_REPORT_SCHEMA_V1,
    };
    use crate::world::spatial::{SurfaceGeometryKind, SurfaceRef, SPHERICAL_SURFACE_SCHEMA_V1};

    fn surface_ref() -> SurfaceRef {
        SurfaceRef::new(
            SurfaceGeometryKind::SphericalV1,
            SPHERICAL_SURFACE_SCHEMA_V1,
            12,
            30,
            [11; 32],
        )
        .unwrap()
    }

    fn id(name: &str) -> QualityMetricId {
        QualityMetricId::new("quality", name, 1).unwrap()
    }

    #[test]
    fn builder_canonicalizes_order_and_rejects_duplicates() {
        let mut builder = NaturalQualityReportBuilder::new(surface_ref());
        builder
            .record_at_most(id("z-last"), 0.25, 32, 0.35)
            .unwrap();
        builder
            .record_between(id("a-first"), 0.38, 32, 0.30, 0.45)
            .unwrap();
        let report = builder.finish().unwrap();
        assert_eq!(report.schema_version(), NATURAL_QUALITY_REPORT_SCHEMA_V1);
        assert_eq!(report.metrics()[0].id().name(), "a-first");
        assert_eq!(report.metrics()[1].id().name(), "z-last");

        let mut duplicate = NaturalQualityReportBuilder::new(surface_ref());
        duplicate.record_at_most(id("same"), 1.0, 1, 2.0).unwrap();
        duplicate.record_at_most(id("same"), 1.0, 1, 2.0).unwrap();
        assert!(duplicate.finish().is_err());
    }

    #[test]
    fn accumulator_uses_stable_neumaier_summation_and_handles_no_weight() {
        let mut accumulator = MetricAccumulator::new();
        accumulator.push(1.0e16, 1.0).unwrap();
        accumulator.push(1.0, 1.0).unwrap();
        accumulator.push(-1.0e16, 1.0).unwrap();
        let available = accumulator.finish().unwrap();
        assert_eq!(available.value(), Some(1.0 / 3.0));
        assert_eq!(available.sample_count(), 3);

        let mut empty = MetricAccumulator::new();
        empty.push(1.0, 0.0).unwrap();
        let unavailable = empty.finish().unwrap();
        assert_eq!(unavailable.value(), None);
        assert_eq!(unavailable.sample_count(), 0);
        assert_eq!(
            unavailable.reason(),
            Some("no positive finite sample weight")
        );
    }

    #[test]
    fn accumulator_rejects_non_finite_values_and_negative_or_non_finite_weights() {
        for (value, weight) in [
            (f64::NAN, 1.0),
            (f64::INFINITY, 1.0),
            (1.0, -1.0),
            (1.0, f64::INFINITY),
        ] {
            assert!(MetricAccumulator::new().push(value, weight).is_err());
        }
    }

    #[test]
    fn builder_thresholds_are_inclusive_and_status_is_explicit() {
        let mut builder = NaturalQualityReportBuilder::new(surface_ref());
        builder
            .record_at_most(id("at-most"), 0.35, 1, 0.35)
            .unwrap();
        builder
            .record_observation_at_least(
                id("at-least"),
                MetricObservation::Available {
                    value: 0.75,
                    sample_count: 1,
                },
                0.75,
            )
            .unwrap();
        builder
            .record_between(id("between-min"), 0.30, 1, 0.30, 0.45)
            .unwrap();
        builder
            .record_between(id("between-max"), 0.45, 1, 0.30, 0.45)
            .unwrap();
        builder.record_at_most(id("fail"), 0.36, 1, 0.35).unwrap();
        builder.record_unbounded(id("unbounded"), 12.0, 1).unwrap();

        let report = builder.finish().unwrap();
        for metric in report.metrics() {
            let expected = if metric.id().name() == "fail" {
                QualityMetricStatus::Fail
            } else {
                QualityMetricStatus::Pass
            };
            assert_eq!(metric.status(), expected, "{}", metric.id().name());
        }
    }

    #[test]
    fn builder_preserves_accumulated_available_and_unavailable_observations() {
        let mut builder = NaturalQualityReportBuilder::new(surface_ref());
        builder
            .record_observation_at_least(
                id("observed-min"),
                MetricObservation::Available {
                    value: 0.75,
                    sample_count: 3,
                },
                0.75,
            )
            .unwrap();
        builder
            .record_observation_between(
                id("observed-range"),
                MetricObservation::Available {
                    value: 0.50,
                    sample_count: 3,
                },
                0.25,
                0.75,
            )
            .unwrap();
        builder
            .record_observation_at_most(
                id("observed-empty"),
                MetricAccumulator::new().finish().unwrap(),
                0.10,
            )
            .unwrap();

        let report = builder.finish().unwrap();
        assert_eq!(report.metrics()[0].id().name(), "observed-empty");
        assert_eq!(
            report.metrics()[0].status(),
            QualityMetricStatus::Unavailable
        );
        assert_eq!(
            report.metrics()[0].reason(),
            Some("no positive finite sample weight")
        );
        assert!(report.metrics()[1..]
            .iter()
            .all(|metric| metric.status() == QualityMetricStatus::Pass));
    }

    #[test]
    fn weighted_fraction_and_jaccard_preserve_empty_semantics() {
        let fraction = area_weighted_fraction(&[true, false, true], &[1.0, 2.0, 3.0]).unwrap();
        assert_eq!(fraction.value(), Some(2.0 / 3.0));
        assert_eq!(fraction.sample_count(), 3);

        let jaccard =
            jaccard_fraction(&[true, false, true], &[true, true, false], &[1.0, 2.0, 3.0]).unwrap();
        assert_eq!(jaccard.value(), Some(1.0 / 6.0));

        let empty = jaccard_fraction(&[false, false], &[false, false], &[1.0, 2.0]).unwrap();
        assert_eq!(empty.value(), None);
        assert_ne!(empty.value(), Some(1.0));
        assert!(jaccard_fraction(&[true], &[true, false], &[1.0]).is_err());
    }
}
