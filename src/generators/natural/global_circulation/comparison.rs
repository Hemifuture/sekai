use super::{
    ClimateIntegratorDiagnostics, ClimateIntegratorError, ExplicitRk3Integrator,
    ImexCrankNicolsonIntegrator, LayeredClimateState, SplitExplicitRk3Integrator,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::CubedSphereGrid;
use crate::world::natural::{PlanetForcing, ProductionIntegratorId};

/// The only integrator allowed to own product P4/P5 climate snapshots.
///
/// This choice is frozen by the Release C1/C2 comparison corpus. IMEX remains
/// independently runnable as a rejected same-equation comparison strategy.
pub const SELECTED_PRODUCTION_INTEGRATOR: ProductionIntegratorId =
    ProductionIntegratorId::SplitExplicitRk3V1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClimateAgreementThresholds {
    minimum_vector_correlation: f64,
    maximum_vector_normalized_rmse: f64,
    minimum_scalar_correlation: f64,
    maximum_scalar_absolute_bias: f64,
}

impl ClimateAgreementThresholds {
    pub const LOCKED: Self = Self {
        minimum_vector_correlation: 0.995,
        maximum_vector_normalized_rmse: 0.05,
        minimum_scalar_correlation: 0.999,
        maximum_scalar_absolute_bias: 0.1,
    };
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClimateAgreementFailure {
    metric: &'static str,
    found: f64,
    required: f64,
}

impl ClimateAgreementFailure {
    pub const fn metric(&self) -> &'static str {
        self.metric
    }

    pub const fn found(&self) -> f64 {
        self.found
    }

    pub const fn required(&self) -> f64 {
        self.required
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClimateStateComparison {
    vector_correlation: f64,
    vector_normalized_rmse: f64,
    scalar_correlation: f64,
    scalar_absolute_bias: f64,
    failures: Vec<ClimateAgreementFailure>,
}

impl ClimateStateComparison {
    pub fn qualifies(&self) -> bool {
        self.failures.is_empty()
    }

    pub const fn vector_correlation(&self) -> f64 {
        self.vector_correlation
    }

    pub const fn vector_normalized_rmse(&self) -> f64 {
        self.vector_normalized_rmse
    }

    pub const fn scalar_correlation(&self) -> f64 {
        self.scalar_correlation
    }

    pub const fn scalar_absolute_bias(&self) -> f64 {
        self.scalar_absolute_bias
    }

    pub fn failures(&self) -> &[ClimateAgreementFailure] {
        &self.failures
    }
}

pub fn compare_climate_states(
    grid: &CubedSphereGrid,
    reference: &LayeredClimateState,
    candidate: &LayeredClimateState,
    thresholds: ClimateAgreementThresholds,
) -> Result<ClimateStateComparison, ClimateIntegratorError> {
    reference.validate_against(grid)?;
    candidate.validate_against(grid)?;
    if reference.profile() != candidate.profile() {
        return Err(ClimateIntegratorError::StateMismatch);
    }
    let (reference_vectors, candidate_vectors) = flatten_vectors(reference, candidate);
    let (reference_scalars, candidate_scalars) = flatten_scalars(reference, candidate);
    let vector_correlation = cosine_agreement(&reference_vectors, &candidate_vectors);
    let vector_normalized_rmse = normalized_rmse(&reference_vectors, &candidate_vectors);
    let scalar_correlation = pearson_agreement(&reference_scalars, &candidate_scalars);
    let scalar_absolute_bias = area_neutral_absolute_bias(&reference_scalars, &candidate_scalars);
    let mut failures = Vec::new();
    if vector_correlation < thresholds.minimum_vector_correlation {
        failures.push(ClimateAgreementFailure {
            metric: "vector_correlation",
            found: vector_correlation,
            required: thresholds.minimum_vector_correlation,
        });
    }
    if vector_normalized_rmse > thresholds.maximum_vector_normalized_rmse {
        failures.push(ClimateAgreementFailure {
            metric: "vector_normalized_rmse",
            found: vector_normalized_rmse,
            required: thresholds.maximum_vector_normalized_rmse,
        });
    }
    if scalar_correlation < thresholds.minimum_scalar_correlation {
        failures.push(ClimateAgreementFailure {
            metric: "scalar_correlation",
            found: scalar_correlation,
            required: thresholds.minimum_scalar_correlation,
        });
    }
    if scalar_absolute_bias > thresholds.maximum_scalar_absolute_bias {
        failures.push(ClimateAgreementFailure {
            metric: "scalar_absolute_bias",
            found: scalar_absolute_bias,
            required: thresholds.maximum_scalar_absolute_bias,
        });
    }
    Ok(ClimateStateComparison {
        vector_correlation,
        vector_normalized_rmse,
        scalar_correlation,
        scalar_absolute_bias,
        failures,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateIntegratorComparison {
    agreement: Option<ClimateStateComparison>,
    diagnostics: ClimateIntegratorDiagnostics,
    integration_failure: Option<String>,
}

impl CandidateIntegratorComparison {
    pub fn qualifies(&self) -> bool {
        self.integration_failure.is_none()
            && self
                .agreement
                .as_ref()
                .is_some_and(ClimateStateComparison::qualifies)
    }

    pub const fn agreement(&self) -> Option<&ClimateStateComparison> {
        self.agreement.as_ref()
    }

    pub const fn diagnostics(&self) -> ClimateIntegratorDiagnostics {
        self.diagnostics
    }

    pub const fn final_linear_relative_residual(&self) -> f64 {
        self.diagnostics.final_linear_relative_residual()
    }

    pub fn integration_failure(&self) -> Option<&str> {
        self.integration_failure.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionCandidateSelection {
    Selected(ProductionIntegratorId),
    NoQualifiedCandidate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntegratorComparisonReport {
    reference_steps: u32,
    imex: CandidateIntegratorComparison,
    split_explicit: CandidateIntegratorComparison,
    selection: ProductionCandidateSelection,
}

impl IntegratorComparisonReport {
    pub const fn reference_steps(&self) -> u32 {
        self.reference_steps
    }

    pub const fn imex(&self) -> &CandidateIntegratorComparison {
        &self.imex
    }

    pub const fn split_explicit(&self) -> &CandidateIntegratorComparison {
        &self.split_explicit
    }

    pub const fn selection(&self) -> ProductionCandidateSelection {
        self.selection
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_integrator_comparison(
    grid: &CubedSphereGrid,
    initial: &LayeredClimateState,
    forcing: &PlanetForcing,
    ocean_edge_permeability: &[f32],
    month: usize,
    horizon_seconds: f64,
    reference_step_seconds: f64,
    cancellation: &BuildCancellation,
) -> Result<IntegratorComparisonReport, ClimateIntegratorError> {
    if !horizon_seconds.is_finite() || horizon_seconds <= 0.0 {
        return Err(ClimateIntegratorError::InvalidTimeStep {
            found: horizon_seconds,
        });
    }
    if !reference_step_seconds.is_finite() || reference_step_seconds <= 0.0 {
        return Err(ClimateIntegratorError::InvalidFastStep {
            found: reference_step_seconds,
        });
    }
    let step_count_f64 = (horizon_seconds / reference_step_seconds).ceil().max(1.0);
    if step_count_f64 > f64::from(u32::MAX) {
        return Err(ClimateIntegratorError::InvalidTimeStep {
            found: horizon_seconds,
        });
    }
    let reference_steps = step_count_f64 as u32;
    let exact_reference_step = horizon_seconds / f64::from(reference_steps);
    let explicit = ExplicitRk3Integrator::new(grid);
    let mut reference = initial.clone();
    for _ in 0..reference_steps {
        reference = explicit
            .advance(
                &reference,
                forcing,
                ocean_edge_permeability,
                month,
                exact_reference_step,
                cancellation,
            )?
            .into_state();
    }
    let imex_attempt = ImexCrankNicolsonIntegrator::new(grid, 32, 1.0e-6)?.advance(
        initial,
        forcing,
        ocean_edge_permeability,
        month,
        horizon_seconds,
        cancellation,
    );
    let split_attempt = SplitExplicitRk3Integrator::new(grid, reference_step_seconds)?.advance(
        initial,
        forcing,
        ocean_edge_permeability,
        month,
        horizon_seconds,
        cancellation,
    );
    let thresholds = ClimateAgreementThresholds::LOCKED;
    let imex = candidate_comparison(grid, &reference, imex_attempt, thresholds)?;
    let split_explicit = candidate_comparison(grid, &reference, split_attempt, thresholds)?;
    let selection = if split_explicit.qualifies()
        && (!imex.qualifies()
            || split_explicit.diagnostics.tendency_evaluations()
                <= imex.diagnostics.tendency_evaluations())
    {
        ProductionCandidateSelection::Selected(ProductionIntegratorId::SplitExplicitRk3V1)
    } else if imex.qualifies() {
        ProductionCandidateSelection::Selected(ProductionIntegratorId::ImexCrankNicolsonV1)
    } else {
        ProductionCandidateSelection::NoQualifiedCandidate
    };
    Ok(IntegratorComparisonReport {
        reference_steps,
        imex,
        split_explicit,
        selection,
    })
}

fn candidate_comparison(
    grid: &CubedSphereGrid,
    reference: &LayeredClimateState,
    attempt: Result<super::ClimateStepResult, ClimateIntegratorError>,
    thresholds: ClimateAgreementThresholds,
) -> Result<CandidateIntegratorComparison, ClimateIntegratorError> {
    match attempt {
        Ok(result) => Ok(CandidateIntegratorComparison {
            agreement: Some(compare_climate_states(
                grid,
                reference,
                result.state(),
                thresholds,
            )?),
            diagnostics: result.diagnostics(),
            integration_failure: None,
        }),
        Err(ClimateIntegratorError::Cancelled) => Err(ClimateIntegratorError::Cancelled),
        Err(error) => {
            let diagnostics = match &error {
                ClimateIntegratorError::LinearSolveNotConverged {
                    iterations,
                    residual,
                    ..
                } => ClimateIntegratorDiagnostics::imex(0, *iterations, 1.0, *residual, 0.0),
                _ => ClimateIntegratorDiagnostics::default(),
            };
            Ok(CandidateIntegratorComparison {
                agreement: None,
                diagnostics,
                integration_failure: Some(error.to_string()),
            })
        }
    }
}

fn flatten_vectors(
    reference: &LayeredClimateState,
    candidate: &LayeredClimateState,
) -> (Vec<f64>, Vec<f64>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for role in reference.active_roles() {
        for (reference, candidate) in reference
            .velocity_m_s(*role)
            .expect("active role")
            .iter()
            .zip(candidate.velocity_m_s(*role).expect("active role"))
        {
            left.extend(reference.iter().map(|value| f64::from(*value)));
            right.extend(candidate.iter().map(|value| f64::from(*value)));
        }
    }
    (left, right)
}

fn flatten_scalars(
    reference: &LayeredClimateState,
    candidate: &LayeredClimateState,
) -> (Vec<f64>, Vec<f64>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for role in reference.active_roles() {
        append_pair(
            &mut left,
            &mut right,
            reference.height_anomaly_m(*role).expect("active role"),
            candidate.height_anomaly_m(*role).expect("active role"),
        );
        append_pair(
            &mut left,
            &mut right,
            reference.temperature_c(*role).expect("active role"),
            candidate.temperature_c(*role).expect("active role"),
        );
    }
    append_pair(
        &mut left,
        &mut right,
        reference.specific_humidity(),
        candidate.specific_humidity(),
    );
    if let (Some(reference), Some(candidate)) = (
        reference.deep_ocean_temperature_c(),
        candidate.deep_ocean_temperature_c(),
    ) {
        append_pair(&mut left, &mut right, reference, candidate);
    }
    (left, right)
}

fn append_pair(left: &mut Vec<f64>, right: &mut Vec<f64>, a: &[f32], b: &[f32]) {
    left.extend(a.iter().map(|value| f64::from(*value)));
    right.extend(b.iter().map(|value| f64::from(*value)));
}

fn cosine_agreement(reference: &[f64], candidate: &[f64]) -> f64 {
    if reference == candidate {
        return 1.0;
    }
    let denominator = norm(reference) * norm(candidate);
    if denominator == 0.0 {
        0.0
    } else {
        (dot(reference, candidate) / denominator).clamp(-1.0, 1.0)
    }
}

fn pearson_agreement(reference: &[f64], candidate: &[f64]) -> f64 {
    if reference == candidate {
        return 1.0;
    }
    let reference_mean = reference.iter().sum::<f64>() / reference.len().max(1) as f64;
    let candidate_mean = candidate.iter().sum::<f64>() / candidate.len().max(1) as f64;
    let centered_reference = reference
        .iter()
        .map(|value| value - reference_mean)
        .collect::<Vec<_>>();
    let centered_candidate = candidate
        .iter()
        .map(|value| value - candidate_mean)
        .collect::<Vec<_>>();
    cosine_agreement(&centered_reference, &centered_candidate)
}

fn normalized_rmse(reference: &[f64], candidate: &[f64]) -> f64 {
    let squared_error = reference
        .iter()
        .zip(candidate)
        .map(|(reference, candidate)| (candidate - reference).powi(2))
        .sum::<f64>();
    let reference_energy = dot(reference, reference);
    if squared_error == 0.0 {
        0.0
    } else if reference_energy == 0.0 {
        f64::INFINITY
    } else {
        (squared_error / reference_energy).sqrt()
    }
}

fn area_neutral_absolute_bias(reference: &[f64], candidate: &[f64]) -> f64 {
    if reference.is_empty() {
        return 0.0;
    }
    reference
        .iter()
        .zip(candidate)
        .map(|(reference, candidate)| candidate - reference)
        .sum::<f64>()
        .abs()
        / reference.len() as f64
}

fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum()
}

fn norm(values: &[f64]) -> f64 {
    dot(values, values).sqrt()
}
