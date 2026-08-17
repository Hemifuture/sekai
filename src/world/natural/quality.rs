use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{SurfaceRef, SurfaceRefError};

/// The only supported serialized natural-quality report schema.
pub const NATURAL_QUALITY_REPORT_SCHEMA_V1: u16 = 1;

const MAX_QUALITY_METRICS: usize = 4_096;
const MAX_IDENTIFIER_COMPONENT_BYTES: usize = 128;

/// A stable, versioned identity for one scientific quality metric.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct QualityMetricId {
    namespace: String,
    name: String,
    version: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QualityMetricIdWire {
    namespace: String,
    name: String,
    version: u16,
}

impl QualityMetricId {
    /// Constructs a metric identity from canonical lowercase ASCII components.
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        version: u16,
    ) -> Result<Self, NaturalQualityValidationError> {
        let id = Self {
            namespace: namespace.into(),
            name: name.into(),
            version,
        };
        id.validate()?;
        Ok(id)
    }

    fn validate(&self) -> Result<(), NaturalQualityValidationError> {
        validate_identifier_component("namespace", &self.namespace)?;
        validate_identifier_component("name", &self.name)?;
        if self.version == 0 {
            return Err(NaturalQualityValidationError::ZeroMetricVersion);
        }
        Ok(())
    }

    /// Returns the stable metric namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Returns the stable metric name within its namespace.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the semantic version of this metric definition.
    pub const fn version(&self) -> u16 {
        self.version
    }
}

impl<'de> Deserialize<'de> for QualityMetricId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = QualityMetricIdWire::deserialize(deserializer)?;
        Self::new(wire.namespace, wire.name, wire.version).map_err(D::Error::custom)
    }
}

/// The explicitly recorded outcome of evaluating one metric against its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityMetricStatus {
    /// The available finite value lies inside the inclusive bounds.
    Pass,
    /// The available finite value lies outside the inclusive bounds.
    Fail,
    /// The metric could not be evaluated from the supplied samples.
    Unavailable,
}

/// Inclusive finite acceptance bounds for a quality metric.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct QualityBounds {
    min: Option<f64>,
    max: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QualityBoundsWire {
    min: Option<f64>,
    max: Option<f64>,
}

impl QualityBounds {
    /// Constructs an evidence-only interval that accepts every finite value.
    pub const fn unbounded() -> Self {
        Self {
            min: None,
            max: None,
        }
    }

    /// Constructs a lower-bounded interval.
    pub fn at_least(min: f64) -> Result<Self, NaturalQualityValidationError> {
        Self::new(Some(min), None)
    }

    /// Constructs an upper-bounded interval.
    pub fn at_most(max: f64) -> Result<Self, NaturalQualityValidationError> {
        Self::new(None, Some(max))
    }

    /// Constructs a closed interval.
    pub fn between(min: f64, max: f64) -> Result<Self, NaturalQualityValidationError> {
        Self::new(Some(min), Some(max))
    }

    fn new(min: Option<f64>, max: Option<f64>) -> Result<Self, NaturalQualityValidationError> {
        let bounds = Self { min, max };
        bounds.validate()?;
        Ok(bounds)
    }

    fn validate(&self) -> Result<(), NaturalQualityValidationError> {
        for (field, value) in [("min", self.min), ("max", self.max)] {
            if value.is_some_and(|value| !value.is_finite()) {
                return Err(NaturalQualityValidationError::NonFiniteBound { field });
            }
        }
        if let (Some(min), Some(max)) = (self.min, self.max) {
            if min > max {
                return Err(NaturalQualityValidationError::InvertedBounds { min, max });
            }
        }
        Ok(())
    }

    fn contains(self, value: f64) -> bool {
        self.min.is_none_or(|min| value >= min) && self.max.is_none_or(|max| value <= max)
    }

    /// Returns the inclusive lower endpoint, if one exists.
    pub const fn min(self) -> Option<f64> {
        self.min
    }

    /// Returns the inclusive upper endpoint, if one exists.
    pub const fn max(self) -> Option<f64> {
        self.max
    }
}

impl<'de> Deserialize<'de> for QualityBounds {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = QualityBoundsWire::deserialize(deserializer)?;
        Self::new(wire.min, wire.max).map_err(D::Error::custom)
    }
}

/// One validated quality measurement and its declared acceptance decision.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QualityMetric {
    id: QualityMetricId,
    status: QualityMetricStatus,
    value: Option<f64>,
    sample_count: u32,
    bounds: QualityBounds,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QualityMetricWire {
    id: QualityMetricId,
    status: QualityMetricStatus,
    value: Option<f64>,
    sample_count: u32,
    bounds: QualityBounds,
    reason: Option<String>,
}

impl QualityMetric {
    /// Constructs a metric only when status, value, bounds, and samples agree.
    pub fn new(
        id: QualityMetricId,
        status: QualityMetricStatus,
        value: Option<f64>,
        sample_count: u32,
        bounds: QualityBounds,
        reason: Option<String>,
    ) -> Result<Self, NaturalQualityValidationError> {
        let metric = Self {
            id,
            status,
            value,
            sample_count,
            bounds,
            reason,
        };
        metric.validate()?;
        Ok(metric)
    }

    fn validate(&self) -> Result<(), NaturalQualityValidationError> {
        self.id.validate()?;
        self.bounds.validate()?;

        match self.status {
            QualityMetricStatus::Pass | QualityMetricStatus::Fail => {
                let value = self
                    .value
                    .ok_or(NaturalQualityValidationError::AvailableMetricMissingValue)?;
                if !value.is_finite() {
                    return Err(NaturalQualityValidationError::NonFiniteMetricValue);
                }
                if self.sample_count == 0 {
                    return Err(NaturalQualityValidationError::AvailableMetricHasNoSamples);
                }
                if self.reason.is_some() {
                    return Err(NaturalQualityValidationError::AvailableMetricHasReason);
                }
                let inside = self.bounds.contains(value);
                if (self.status == QualityMetricStatus::Pass) != inside {
                    return Err(NaturalQualityValidationError::StatusDoesNotMatchBounds {
                        status: self.status,
                        value,
                    });
                }
            }
            QualityMetricStatus::Unavailable => {
                if self.value.is_some() {
                    return Err(NaturalQualityValidationError::UnavailableMetricHasValue);
                }
                if self.sample_count != 0 {
                    return Err(NaturalQualityValidationError::UnavailableMetricHasSamples {
                        found: self.sample_count,
                    });
                }
                if self
                    .reason
                    .as_deref()
                    .is_none_or(|reason| reason.trim().is_empty())
                {
                    return Err(NaturalQualityValidationError::UnavailableMetricMissingReason);
                }
            }
        }
        Ok(())
    }

    /// Returns the stable identity of this metric.
    pub const fn id(&self) -> &QualityMetricId {
        &self.id
    }

    /// Returns the explicit acceptance outcome.
    pub const fn status(&self) -> QualityMetricStatus {
        self.status
    }

    /// Returns the finite value, or `None` when unavailable.
    pub const fn value(&self) -> Option<f64> {
        self.value
    }

    /// Returns the number of contributing samples.
    pub const fn sample_count(&self) -> u32 {
        self.sample_count
    }

    /// Returns the inclusive acceptance bounds.
    pub const fn bounds(&self) -> QualityBounds {
        self.bounds
    }

    /// Returns the explanation recorded for an unavailable metric.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

impl<'de> Deserialize<'de> for QualityMetric {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = QualityMetricWire::deserialize(deserializer)?;
        Self::new(
            wire.id,
            wire.status,
            wire.value,
            wire.sample_count,
            wire.bounds,
            wire.reason,
        )
        .map_err(D::Error::custom)
    }
}

/// A deterministic, surface-bound inventory of natural-world quality results.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NaturalQualityReport {
    schema_version: u16,
    surface_ref: SurfaceRef,
    metrics: Vec<QualityMetric>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NaturalQualityReportWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    #[serde(deserialize_with = "deserialize_quality_metrics")]
    metrics: Vec<QualityMetric>,
}

fn deserialize_quality_metrics<'de, D>(deserializer: D) -> Result<Vec<QualityMetric>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_QUALITY_METRICS>(deserializer)
}

impl NaturalQualityReport {
    /// Constructs a canonical report, sorting metrics and rejecting duplicate IDs.
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        mut metrics: Vec<QualityMetric>,
    ) -> Result<Self, NaturalQualityValidationError> {
        if schema_version != NATURAL_QUALITY_REPORT_SCHEMA_V1 {
            return Err(NaturalQualityValidationError::UnsupportedSchema {
                found: schema_version,
                supported: NATURAL_QUALITY_REPORT_SCHEMA_V1,
            });
        }
        surface_ref.validate()?;
        if metrics.len() > MAX_QUALITY_METRICS {
            return Err(NaturalQualityValidationError::TooManyMetrics {
                found: metrics.len(),
                max: MAX_QUALITY_METRICS,
            });
        }
        for metric in &metrics {
            metric.validate()?;
        }
        metrics.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        if let Some(duplicate) = metrics
            .windows(2)
            .find(|pair| pair[0].id == pair[1].id)
            .map(|pair| pair[0].id.clone())
        {
            return Err(NaturalQualityValidationError::DuplicateMetric { id: duplicate });
        }
        Ok(Self {
            schema_version,
            surface_ref,
            metrics,
        })
    }

    /// Returns the report schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact authoritative surface evaluated by this report.
    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    /// Returns the metrics in canonical identity order.
    pub fn metrics(&self) -> &[QualityMetric] {
        &self.metrics
    }
}

impl<'de> Deserialize<'de> for NaturalQualityReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = NaturalQualityReportWire::deserialize(deserializer)?;
        Self::new(wire.schema_version, wire.surface_ref, wire.metrics).map_err(D::Error::custom)
    }
}

fn validate_identifier_component(
    field: &'static str,
    value: &str,
) -> Result<(), NaturalQualityValidationError> {
    let bytes = value.as_bytes();
    if !(1..=MAX_IDENTIFIER_COMPONENT_BYTES).contains(&bytes.len()) {
        return Err(NaturalQualityValidationError::IdentifierLengthOutOfRange {
            field,
            found: bytes.len(),
            max: MAX_IDENTIFIER_COMPONENT_BYTES,
        });
    }
    let is_alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    if !is_alphanumeric(bytes[0]) || !is_alphanumeric(bytes[bytes.len() - 1]) {
        return Err(NaturalQualityValidationError::InvalidIdentifierEndpoint { field });
    }
    if let Some((index, found)) = bytes
        .iter()
        .copied()
        .enumerate()
        .find(|(_, byte)| !is_alphanumeric(*byte) && !matches!(*byte, b'-' | b'_' | b'.'))
    {
        return Err(NaturalQualityValidationError::InvalidIdentifierCharacter {
            field,
            index,
            found,
        });
    }
    Ok(())
}

/// Errors returned when a quality identity, measurement, or report is contradictory.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum NaturalQualityValidationError {
    #[error("unsupported natural quality report schema {found}; supported version is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("invalid natural quality surface: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    #[error("quality metric identifier {field} has {found} bytes; expected 1..={max}")]
    IdentifierLengthOutOfRange {
        field: &'static str,
        found: usize,
        max: usize,
    },
    #[error("quality metric identifier {field} must start and end with a lowercase ASCII letter or digit")]
    InvalidIdentifierEndpoint { field: &'static str },
    #[error(
        "quality metric identifier {field} contains invalid byte {found:#04x} at index {index}"
    )]
    InvalidIdentifierCharacter {
        field: &'static str,
        index: usize,
        found: u8,
    },
    #[error("quality metric identifier versions must be nonzero")]
    ZeroMetricVersion,
    #[error("quality bound {field} must be finite")]
    NonFiniteBound { field: &'static str },
    #[error("quality bounds are inverted: minimum {min} exceeds maximum {max}")]
    InvertedBounds { min: f64, max: f64 },
    #[error("an available quality metric must contain a value")]
    AvailableMetricMissingValue,
    #[error("an available quality metric value must be finite")]
    NonFiniteMetricValue,
    #[error("an available quality metric must contain at least one sample")]
    AvailableMetricHasNoSamples,
    #[error("an available quality metric cannot contain an unavailable reason")]
    AvailableMetricHasReason,
    #[error("quality metric status {status:?} contradicts value {value} and its bounds")]
    StatusDoesNotMatchBounds {
        status: QualityMetricStatus,
        value: f64,
    },
    #[error("an unavailable quality metric cannot contain a value")]
    UnavailableMetricHasValue,
    #[error("an unavailable quality metric must have zero samples, found {found}")]
    UnavailableMetricHasSamples { found: u32 },
    #[error("an unavailable quality metric must contain a non-empty reason")]
    UnavailableMetricMissingReason,
    #[error("natural quality report contains {found} metrics; maximum is {max}")]
    TooManyMetrics { found: usize, max: usize },
    #[error("natural quality report contains duplicate metric {id:?}")]
    DuplicateMetric { id: QualityMetricId },
}
