use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::world::natural::{
    CirculationSnapshot, CirculationSnapshotError, CirculationSolveStats, CirculationSolverId,
    CLIMATE_MONTH_COUNT,
};

use super::{CirculationFixture, CubedSphereGrid};

const VECTOR_RMS_EPSILON: f64 = 1.0e-12;
const SCALAR_MEAN_EPSILON: f64 = 1.0e-12;
const COSINE_45_DEGREES: f64 = std::f64::consts::FRAC_1_SQRT_2;
const WIND_DIRECTION_THRESHOLD_M_S: f64 = 0.1;
const CURRENT_DIRECTION_THRESHOLD_M_S: f64 = 0.01;

/// Area-weighted agreement for one monthly tangent-vector field.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VectorAgreement {
    pub vector_correlation: f64,
    pub normalized_rmse: f64,
    pub direction_agreement: f64,
    pub reference_rms: f64,
    pub direction_sampled_area_fraction: f64,
}

/// Area-weighted agreement for one monthly scalar field.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScalarAgreement {
    pub correlation: f64,
    pub rmse: f64,
    pub bias: f64,
    pub total_relative_bias: f64,
    pub candidate_area_mean: f64,
    pub reference_area_mean: f64,
}

/// Every compared prognostic or diagnostic field for one climatological month.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonthlyAgreement {
    pub month: u8,
    pub wind: VectorAgreement,
    pub ocean_current: VectorAgreement,
    pub air_temperature: ScalarAgreement,
    pub surface_temperature: ScalarAgreement,
    pub specific_humidity: ScalarAgreement,
    pub precipitation: ScalarAgreement,
    pub atmosphere_height: ScalarAgreement,
    pub sea_surface_height: ScalarAgreement,
}

/// The direction in which an eligibility threshold must be satisfied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EligibilityRule {
    AtLeast,
    AtMost,
    AbsoluteAtMost,
}

/// One failed published WYSIWYG threshold with its unmodified observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EligibilityFailure {
    pub metric: String,
    pub month: Option<u8>,
    pub observed: f64,
    pub threshold: f64,
    pub rule: EligibilityRule,
}

/// Detailed outcome of applying the published WYSIWYG thresholds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WysiwygEligibility {
    pub eligible: bool,
    pub failures: Vec<EligibilityFailure>,
}

impl WysiwygEligibility {
    pub fn evaluate(monthly: &[MonthlyAgreement]) -> Self {
        let mut failures = Vec::new();
        if monthly.len() != CLIMATE_MONTH_COUNT {
            failures.push(EligibilityFailure {
                metric: "month_count".to_owned(),
                month: None,
                observed: monthly.len() as f64,
                threshold: CLIMATE_MONTH_COUNT as f64,
                rule: EligibilityRule::AtLeast,
            });
        }

        for agreement in monthly {
            require_at_least(
                &mut failures,
                "wind.vector_correlation",
                agreement.month,
                agreement.wind.vector_correlation,
                0.95,
            );
            require_at_most(
                &mut failures,
                "wind.normalized_rmse",
                agreement.month,
                agreement.wind.normalized_rmse,
                0.20,
            );
            require_at_least(
                &mut failures,
                "wind.direction_agreement",
                agreement.month,
                agreement.wind.direction_agreement,
                0.90,
            );
            require_at_least(
                &mut failures,
                "ocean_current.vector_correlation",
                agreement.month,
                agreement.ocean_current.vector_correlation,
                0.90,
            );
            require_at_most(
                &mut failures,
                "ocean_current.normalized_rmse",
                agreement.month,
                agreement.ocean_current.normalized_rmse,
                0.30,
            );
            require_at_least(
                &mut failures,
                "ocean_current.direction_agreement",
                agreement.month,
                agreement.ocean_current.direction_agreement,
                0.85,
            );
            require_at_least(
                &mut failures,
                "air_temperature.correlation",
                agreement.month,
                agreement.air_temperature.correlation,
                0.98,
            );
            require_absolute_at_most(
                &mut failures,
                "air_temperature.bias_c",
                agreement.month,
                agreement.air_temperature.bias,
                0.5,
            );
            require_at_least(
                &mut failures,
                "precipitation.correlation",
                agreement.month,
                agreement.precipitation.correlation,
                0.95,
            );
        }

        let candidate_annual_precipitation = monthly
            .iter()
            .map(|agreement| agreement.precipitation.candidate_area_mean)
            .sum::<f64>();
        let reference_annual_precipitation = monthly
            .iter()
            .map(|agreement| agreement.precipitation.reference_area_mean)
            .sum::<f64>();
        let annual_precipitation_bias = relative_bias(
            candidate_annual_precipitation,
            reference_annual_precipitation,
        );
        require_absolute_at_most_without_month(
            &mut failures,
            "annual_precipitation.total_relative_bias",
            annual_precipitation_bias,
            0.02,
        );

        Self {
            eligible: failures.is_empty(),
            failures,
        }
    }
}

/// Immutable metrics and source diagnostics for one snapshot pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub cell_count: u32,
    pub spec_fingerprint: [u8; 32],
    pub grid_fingerprint: [u8; 32],
    pub forcing_fingerprint: [u8; 32],
    pub candidate_solver_id: CirculationSolverId,
    pub reference_solver_id: CirculationSolverId,
    pub candidate_stats: CirculationSolveStats,
    pub reference_stats: CirculationSolveStats,
    pub monthly: Vec<MonthlyAgreement>,
    pub wysiwyg: WysiwygEligibility,
}

/// A comparison labeled by its deterministic scientific fixture and resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureComparison {
    pub fixture: CirculationFixture,
    pub face_resolution: u16,
    pub report: ComparisonReport,
}

/// Compares two immutable snapshots on their shared closed spherical grid.
pub fn compare_snapshots(
    grid: &CubedSphereGrid,
    candidate: &CirculationSnapshot,
    reference: &CirculationSnapshot,
) -> Result<ComparisonReport, ComparisonError> {
    candidate.validate()?;
    reference.validate()?;
    validate_identity(grid, candidate, reference)?;

    let mut monthly = Vec::with_capacity(CLIMATE_MONTH_COUNT);
    for month in 0..CLIMATE_MONTH_COUNT {
        monthly.push(MonthlyAgreement {
            month: (month + 1) as u8,
            wind: vector_agreement(
                grid,
                candidate.monthly_wind_m_s(),
                reference.monthly_wind_m_s(),
                month,
                WIND_DIRECTION_THRESHOLD_M_S,
            ),
            ocean_current: vector_agreement(
                grid,
                candidate.monthly_ocean_current_m_s(),
                reference.monthly_ocean_current_m_s(),
                month,
                CURRENT_DIRECTION_THRESHOLD_M_S,
            ),
            air_temperature: scalar_agreement(
                grid,
                candidate.monthly_air_temperature_c(),
                reference.monthly_air_temperature_c(),
                month,
            ),
            surface_temperature: scalar_agreement(
                grid,
                candidate.monthly_surface_temperature_c(),
                reference.monthly_surface_temperature_c(),
                month,
            ),
            specific_humidity: scalar_agreement(
                grid,
                candidate.monthly_specific_humidity(),
                reference.monthly_specific_humidity(),
                month,
            ),
            precipitation: scalar_agreement(
                grid,
                candidate.monthly_precipitation_mm_day(),
                reference.monthly_precipitation_mm_day(),
                month,
            ),
            atmosphere_height: scalar_agreement(
                grid,
                candidate.monthly_atmosphere_height_anomaly_m(),
                reference.monthly_atmosphere_height_anomaly_m(),
                month,
            ),
            sea_surface_height: scalar_agreement(
                grid,
                candidate.monthly_sea_surface_height_anomaly_m(),
                reference.monthly_sea_surface_height_anomaly_m(),
                month,
            ),
        });
    }
    let wysiwyg = WysiwygEligibility::evaluate(&monthly);
    Ok(ComparisonReport {
        cell_count: candidate.cell_count(),
        spec_fingerprint: *candidate.spec_fingerprint(),
        grid_fingerprint: *candidate.grid_fingerprint(),
        forcing_fingerprint: *candidate.forcing_fingerprint(),
        candidate_solver_id: candidate.solver_id(),
        reference_solver_id: reference.solver_id(),
        candidate_stats: *candidate.stats(),
        reference_stats: *reference.stats(),
        monthly,
        wysiwyg,
    })
}

fn validate_identity(
    grid: &CubedSphereGrid,
    candidate: &CirculationSnapshot,
    reference: &CirculationSnapshot,
) -> Result<(), ComparisonError> {
    if candidate.cell_count() as usize != grid.cell_count() {
        return Err(ComparisonError::GridCellCountMismatch {
            snapshot: "candidate",
            expected: grid.cell_count(),
            found: candidate.cell_count() as usize,
        });
    }
    if reference.cell_count() as usize != grid.cell_count() {
        return Err(ComparisonError::GridCellCountMismatch {
            snapshot: "reference",
            expected: grid.cell_count(),
            found: reference.cell_count() as usize,
        });
    }
    if candidate.grid_fingerprint() != grid.fingerprint() {
        return Err(ComparisonError::SnapshotGridMismatch {
            snapshot: "candidate",
        });
    }
    if reference.grid_fingerprint() != grid.fingerprint() {
        return Err(ComparisonError::SnapshotGridMismatch {
            snapshot: "reference",
        });
    }
    if candidate.spec_fingerprint() != reference.spec_fingerprint() {
        return Err(ComparisonError::SpecFingerprintMismatch);
    }
    if candidate.grid_fingerprint() != reference.grid_fingerprint() {
        return Err(ComparisonError::GridFingerprintMismatch);
    }
    if candidate.forcing_fingerprint() != reference.forcing_fingerprint() {
        return Err(ComparisonError::ForcingFingerprintMismatch);
    }
    Ok(())
}

fn vector_agreement(
    grid: &CubedSphereGrid,
    candidate: &[[[f32; 3]; CLIMATE_MONTH_COUNT]],
    reference: &[[[f32; 3]; CLIMATE_MONTH_COUNT]],
    month: usize,
    direction_threshold: f64,
) -> VectorAgreement {
    let mut total_area = CompensatedSum::default();
    let mut dot_product = CompensatedSum::default();
    let mut candidate_energy = CompensatedSum::default();
    let mut reference_energy = CompensatedSum::default();
    let mut squared_error = CompensatedSum::default();
    let mut sampled_area = CompensatedSum::default();
    let mut aligned_area = CompensatedSum::default();

    for ((cell, candidate), reference) in grid.cells().iter().zip(candidate).zip(reference) {
        let area = cell.area_m2();
        let candidate = to_f64_vector(candidate[month]);
        let reference = to_f64_vector(reference[month]);
        let dot = vector_dot(candidate, reference);
        let candidate_squared = vector_dot(candidate, candidate);
        let reference_squared = vector_dot(reference, reference);
        let delta = [
            candidate[0] - reference[0],
            candidate[1] - reference[1],
            candidate[2] - reference[2],
        ];
        total_area.add(area);
        dot_product.add(area * dot);
        candidate_energy.add(area * candidate_squared);
        reference_energy.add(area * reference_squared);
        squared_error.add(area * vector_dot(delta, delta));

        let candidate_speed = candidate_squared.sqrt();
        let reference_speed = reference_squared.sqrt();
        if candidate_speed >= direction_threshold && reference_speed >= direction_threshold {
            sampled_area.add(area);
            let cosine = dot / (candidate_speed * reference_speed);
            if cosine >= COSINE_45_DEGREES {
                aligned_area.add(area);
            }
        }
    }

    let area = total_area.total();
    let reference_rms = (reference_energy.total() / area).max(0.0).sqrt();
    let rmse = (squared_error.total() / area).max(0.0).sqrt();
    let denominator = (candidate_energy.total() * reference_energy.total())
        .max(0.0)
        .sqrt();
    let vector_correlation = if denominator > 0.0 {
        (dot_product.total() / denominator).clamp(-1.0, 1.0)
    } else if rmse <= VECTOR_RMS_EPSILON {
        1.0
    } else {
        0.0
    };
    let normalized_rmse = if rmse == 0.0 {
        0.0
    } else {
        rmse / reference_rms.max(VECTOR_RMS_EPSILON)
    };
    let sampled = sampled_area.total();
    let direction_agreement = if sampled > 0.0 {
        (aligned_area.total() / sampled).clamp(0.0, 1.0)
    } else {
        1.0
    };
    VectorAgreement {
        vector_correlation,
        normalized_rmse,
        direction_agreement,
        reference_rms,
        direction_sampled_area_fraction: (sampled / area).clamp(0.0, 1.0),
    }
}

fn scalar_agreement(
    grid: &CubedSphereGrid,
    candidate: &[[f32; CLIMATE_MONTH_COUNT]],
    reference: &[[f32; CLIMATE_MONTH_COUNT]],
    month: usize,
) -> ScalarAgreement {
    let mut total_area = CompensatedSum::default();
    let mut candidate_total = CompensatedSum::default();
    let mut reference_total = CompensatedSum::default();
    for ((cell, candidate), reference) in grid.cells().iter().zip(candidate).zip(reference) {
        let area = cell.area_m2();
        total_area.add(area);
        candidate_total.add(area * f64::from(candidate[month]));
        reference_total.add(area * f64::from(reference[month]));
    }
    let area = total_area.total();
    let candidate_mean = candidate_total.total() / area;
    let reference_mean = reference_total.total() / area;

    let mut covariance = CompensatedSum::default();
    let mut candidate_variance = CompensatedSum::default();
    let mut reference_variance = CompensatedSum::default();
    let mut squared_error = CompensatedSum::default();
    let mut signed_error = CompensatedSum::default();
    for ((cell, candidate), reference) in grid.cells().iter().zip(candidate).zip(reference) {
        let area = cell.area_m2();
        let candidate = f64::from(candidate[month]);
        let reference = f64::from(reference[month]);
        let candidate_anomaly = candidate - candidate_mean;
        let reference_anomaly = reference - reference_mean;
        let delta = candidate - reference;
        covariance.add(area * candidate_anomaly * reference_anomaly);
        candidate_variance.add(area * candidate_anomaly * candidate_anomaly);
        reference_variance.add(area * reference_anomaly * reference_anomaly);
        squared_error.add(area * delta * delta);
        signed_error.add(area * delta);
    }
    let rmse = (squared_error.total() / area).max(0.0).sqrt();
    let denominator = (candidate_variance.total() * reference_variance.total())
        .max(0.0)
        .sqrt();
    let correlation = if denominator > 0.0 {
        (covariance.total() / denominator).clamp(-1.0, 1.0)
    } else if rmse <= SCALAR_MEAN_EPSILON {
        1.0
    } else {
        0.0
    };
    let bias = signed_error.total() / area;
    ScalarAgreement {
        correlation,
        rmse,
        bias,
        total_relative_bias: relative_bias(candidate_mean, reference_mean),
        candidate_area_mean: candidate_mean,
        reference_area_mean: reference_mean,
    }
}

fn relative_bias(candidate: f64, reference: f64) -> f64 {
    let delta = candidate - reference;
    if delta == 0.0 {
        0.0
    } else {
        delta / reference.abs().max(SCALAR_MEAN_EPSILON)
    }
}

fn require_at_least(
    failures: &mut Vec<EligibilityFailure>,
    metric: &str,
    month: u8,
    observed: f64,
    threshold: f64,
) {
    if !observed.is_finite() || observed < threshold {
        failures.push(EligibilityFailure {
            metric: metric.to_owned(),
            month: Some(month),
            observed,
            threshold,
            rule: EligibilityRule::AtLeast,
        });
    }
}

fn require_at_most(
    failures: &mut Vec<EligibilityFailure>,
    metric: &str,
    month: u8,
    observed: f64,
    threshold: f64,
) {
    if !observed.is_finite() || observed > threshold {
        failures.push(EligibilityFailure {
            metric: metric.to_owned(),
            month: Some(month),
            observed,
            threshold,
            rule: EligibilityRule::AtMost,
        });
    }
}

fn require_absolute_at_most(
    failures: &mut Vec<EligibilityFailure>,
    metric: &str,
    month: u8,
    observed: f64,
    threshold: f64,
) {
    if !observed.is_finite() || observed.abs() > threshold {
        failures.push(EligibilityFailure {
            metric: metric.to_owned(),
            month: Some(month),
            observed,
            threshold,
            rule: EligibilityRule::AbsoluteAtMost,
        });
    }
}

fn require_absolute_at_most_without_month(
    failures: &mut Vec<EligibilityFailure>,
    metric: &str,
    observed: f64,
    threshold: f64,
) {
    if !observed.is_finite() || observed.abs() > threshold {
        failures.push(EligibilityFailure {
            metric: metric.to_owned(),
            month: None,
            observed,
            threshold,
            rule: EligibilityRule::AbsoluteAtMost,
        });
    }
}

fn to_f64_vector(value: [f32; 3]) -> [f64; 3] {
    [
        f64::from(value[0]),
        f64::from(value[1]),
        f64::from(value[2]),
    ]
}

fn vector_dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

#[derive(Debug, Clone, Copy, Default)]
struct CompensatedSum {
    sum: f64,
    correction: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) {
        let next = self.sum + value;
        if self.sum.abs() >= value.abs() {
            self.correction += (self.sum - next) + value;
        } else {
            self.correction += (value - next) + self.sum;
        }
        self.sum = next;
    }

    fn total(self) -> f64 {
        self.sum + self.correction
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ComparisonError {
    #[error(transparent)]
    Snapshot(#[from] CirculationSnapshotError),
    #[error("{snapshot} snapshot has {found} cells; grid has {expected}")]
    GridCellCountMismatch {
        snapshot: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("{snapshot} snapshot was produced on a different grid")]
    SnapshotGridMismatch { snapshot: &'static str },
    #[error("snapshots use different circulation specifications")]
    SpecFingerprintMismatch,
    #[error("snapshots use different spherical grids")]
    GridFingerprintMismatch,
    #[error("snapshots use different planetary forcing")]
    ForcingFingerprintMismatch,
}
