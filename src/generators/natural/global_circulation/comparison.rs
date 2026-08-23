use serde::Serialize;

use super::{
    ClimateIntegratorDiagnostics, ClimateIntegratorError, ClimateStepResult,
    ImexCrankNicolsonIntegrator, LayeredClimateState, SplitExplicitRk3Integrator,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::CubedSphereGrid;
use crate::world::natural::{
    ClimateCapabilitySet, ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, PlanetForcing,
    ProductionIntegratorId, CLIMATE_MONTH_COUNT, CLIMATOLOGICAL_YEAR_SECONDS,
};

const FORMATION_COMPARISON_RESIDUAL_TARGET: f64 = 0.24;
const PRODUCTION_SLOW_STEP_SECONDS: f64 = 7_200.0;

/// The only integrator allowed to own product P4/P5 climate snapshots.
///
/// This choice is frozen by the Release C1/C2 comparison corpus. IMEX remains
/// independently runnable as a rejected same-equation comparison strategy.
pub const SELECTED_PRODUCTION_INTEGRATOR: ProductionIntegratorId =
    ProductionIntegratorId::SplitExplicitRk3V1;

/// Locked maximum relative drift for every active layer in the closed annual
/// split-explicit analytic fixture.
pub const CLOSED_ANNUAL_LAYER_MASS_DRIFT_MAX: f64 = 1.0e-8;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LayerMassConservationDiagnostic {
    role: ClimateLayerRole,
    initial_mass_kg: f64,
    signed_mass_change_kg: f64,
    relative_mass_drift: f64,
}

impl LayerMassConservationDiagnostic {
    pub const fn role(&self) -> ClimateLayerRole {
        self.role
    }

    pub const fn initial_mass_kg(&self) -> f64 {
        self.initial_mass_kg
    }

    pub const fn signed_mass_change_kg(&self) -> f64 {
        self.signed_mass_change_kg
    }

    pub const fn relative_mass_drift(&self) -> f64 {
        self.relative_mass_drift
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnnualLayerMassConservationReport {
    months: u8,
    elapsed_seconds: f64,
    maximum_absolute_height_change_m: f64,
    maximum_relative_mass_drift: f64,
    layers: Vec<LayerMassConservationDiagnostic>,
}

impl AnnualLayerMassConservationReport {
    pub const fn months(&self) -> u8 {
        self.months
    }

    pub const fn elapsed_seconds(&self) -> f64 {
        self.elapsed_seconds
    }

    pub const fn maximum_absolute_height_change_m(&self) -> f64 {
        self.maximum_absolute_height_change_m
    }

    pub const fn maximum_relative_mass_drift(&self) -> f64 {
        self.maximum_relative_mass_drift
    }

    pub fn layers(&self) -> &[LayerMassConservationDiagnostic] {
        &self.layers
    }

    pub fn qualifies(&self) -> bool {
        self.months == CLIMATE_MONTH_COUNT as u8
            && self.maximum_absolute_height_change_m > 0.0
            && self.maximum_relative_mass_drift <= CLOSED_ANNUAL_LAYER_MASS_DRIFT_MAX
    }
}

/// Runs the locked nontrivial C2 gravity-wave fixture for one complete
/// climatological year through the selected integrator's closed fast kernel.
pub fn run_closed_split_annual_mass_fixture(
    grid: &CubedSphereGrid,
    cancellation: &BuildCancellation,
) -> Result<AnnualLayerMassConservationReport, ClimateIntegratorError> {
    let count = grid.cell_count();
    let forcing = PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![1.0; count],
        vec![[240.0; CLIMATE_MONTH_COUNT]; count],
        vec![[15.0; CLIMATE_MONTH_COUNT]; count],
        vec![[15.0; CLIMATE_MONTH_COUNT]; count],
        vec![[0.008; CLIMATE_MONTH_COUNT]; count],
    )
    .expect("the locked closed annual fixture forcing is valid");
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let mut initial = LayeredClimateState::from_forcing(grid, &layout, &forcing, 0)?;
    for role in initial.active_roles().to_vec() {
        let amplitude_m = match role {
            ClimateLayerRole::LowerAtmosphere => 25.0,
            ClimateLayerRole::UpperAtmosphere => 12.0,
            ClimateLayerRole::OceanMixedLayer => 1.5,
            ClimateLayerRole::OceanThermocline => 0.75,
            ClimateLayerRole::DeepOceanReservoir => unreachable!(),
        };
        for (cell, anomaly) in grid
            .cells()
            .iter()
            .zip(initial.height_anomaly_m_mut(role).expect("active role"))
        {
            let [x, y, z] = cell.center_unit();
            *anomaly = (amplitude_m * (x + 0.35 * y - 0.2 * z)) as f32;
        }
    }

    let permeability = vec![1.0; grid.edges().len()];
    let integrator = SplitExplicitRk3Integrator::new(grid, 21_600.0)?;
    let mut final_state = initial.clone();
    let month_seconds = CLIMATOLOGICAL_YEAR_SECONDS / CLIMATE_MONTH_COUNT as f64;
    for month in 0..CLIMATE_MONTH_COUNT {
        final_state = integrator
            .advance_closed_no_source(
                &final_state,
                &forcing,
                &permeability,
                month,
                month_seconds,
                cancellation,
            )?
            .into_state();
    }

    let mut layers = Vec::with_capacity(initial.active_roles().len());
    let mut maximum_absolute_height_change_m = 0.0_f64;
    let mut maximum_relative_mass_drift = 0.0_f64;
    for &role in initial.active_roles() {
        let density = layout
            .layers()
            .iter()
            .find(|layer| layer.role() == role)
            .expect("fixed C2 layout contains every active role")
            .density_kg_m3();
        let reference = f64::from(initial.reference_thickness_m(role).expect("active role"));
        let before = initial.height_anomaly_m(role).expect("active role");
        let after = final_state.height_anomaly_m(role).expect("active role");
        let initial_mass_kg = grid
            .cells()
            .iter()
            .zip(before)
            .map(|(cell, anomaly)| cell.area_m2() * density * (reference + f64::from(*anomaly)))
            .sum::<f64>();
        let signed_mass_change_kg = grid
            .cells()
            .iter()
            .zip(before.iter().zip(after))
            .map(|(cell, (before, after))| {
                cell.area_m2() * density * (f64::from(*after) - f64::from(*before))
            })
            .sum::<f64>();
        for (&before, &after) in before.iter().zip(after) {
            maximum_absolute_height_change_m =
                maximum_absolute_height_change_m.max((f64::from(after) - f64::from(before)).abs());
        }
        let relative_mass_drift = signed_mass_change_kg.abs() / initial_mass_kg.abs().max(1.0);
        maximum_relative_mass_drift = maximum_relative_mass_drift.max(relative_mass_drift);
        layers.push(LayerMassConservationDiagnostic {
            role,
            initial_mass_kg,
            signed_mass_change_kg,
            relative_mass_drift,
        });
    }

    Ok(AnnualLayerMassConservationReport {
        months: CLIMATE_MONTH_COUNT as u8,
        elapsed_seconds: CLIMATOLOGICAL_YEAR_SECONDS,
        maximum_absolute_height_change_m,
        maximum_relative_mass_drift,
        layers,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ClimateAgreementThresholds {
    minimum_vector_correlation: f64,
    maximum_vector_normalized_rmse: f64,
    minimum_scalar_correlation: f64,
    maximum_scalar_absolute_bias: f64,
    minimum_precipitation_correlation: f64,
    maximum_annual_precipitation_total_bias_fraction: f64,
}

impl ClimateAgreementThresholds {
    pub const LOCKED: Self = Self {
        minimum_vector_correlation: 0.995,
        maximum_vector_normalized_rmse: 0.05,
        minimum_scalar_correlation: 0.999,
        maximum_scalar_absolute_bias: 0.1,
        minimum_precipitation_correlation: 0.98,
        maximum_annual_precipitation_total_bias_fraction: 0.01,
    };

    pub const fn maximum_annual_precipitation_total_bias_fraction(self) -> f64 {
        self.maximum_annual_precipitation_total_bias_fraction
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClimateAgreementFailure {
    field: &'static str,
    metric: &'static str,
    found: f64,
    required: f64,
}

impl ClimateAgreementFailure {
    pub const fn field(&self) -> &'static str {
        self.field
    }

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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClimateVectorAgreement {
    field: &'static str,
    correlation: f64,
    normalized_rmse: f64,
}

impl ClimateVectorAgreement {
    pub const fn field(&self) -> &'static str {
        self.field
    }

    pub const fn correlation(&self) -> f64 {
        self.correlation
    }

    pub const fn normalized_rmse(&self) -> f64 {
        self.normalized_rmse
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClimateScalarAgreement {
    field: &'static str,
    correlation: f64,
    absolute_area_weighted_bias: f64,
}

impl ClimateScalarAgreement {
    pub const fn field(&self) -> &'static str {
        self.field
    }

    pub const fn correlation(&self) -> f64 {
        self.correlation
    }

    pub const fn absolute_area_weighted_bias(&self) -> f64 {
        self.absolute_area_weighted_bias
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClimatePrecipitationAgreement {
    correlation: f64,
    reference_area_integrated_rate: f64,
    candidate_area_integrated_rate: f64,
}

impl ClimatePrecipitationAgreement {
    pub const fn correlation(&self) -> f64 {
        self.correlation
    }

    pub const fn reference_area_integrated_rate(&self) -> f64 {
        self.reference_area_integrated_rate
    }

    pub const fn candidate_area_integrated_rate(&self) -> f64 {
        self.candidate_area_integrated_rate
    }

    pub fn total_bias_fraction(&self) -> f64 {
        relative_total_bias(
            self.reference_area_integrated_rate,
            self.candidate_area_integrated_rate,
        )
    }
}

/// Per-semantic-field agreement. No velocity or scalar from one physical role
/// can compensate for a failure in another role.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClimateStateComparison {
    vector_fields: Vec<ClimateVectorAgreement>,
    scalar_fields: Vec<ClimateScalarAgreement>,
    precipitation: Option<ClimatePrecipitationAgreement>,
    failures: Vec<ClimateAgreementFailure>,
}

impl ClimateStateComparison {
    pub fn qualifies(&self) -> bool {
        self.failures.is_empty()
    }

    /// Worst named vector correlation, retained for compact diagnostics.
    pub fn vector_correlation(&self) -> f64 {
        self.vector_fields
            .iter()
            .map(ClimateVectorAgreement::correlation)
            .fold(1.0_f64, f64::min)
    }

    /// Worst named vector normalized RMSE, retained for compact diagnostics.
    pub fn vector_normalized_rmse(&self) -> f64 {
        self.vector_fields
            .iter()
            .map(ClimateVectorAgreement::normalized_rmse)
            .fold(0.0_f64, f64::max)
    }

    /// Worst named air/SST correlation.
    pub fn scalar_correlation(&self) -> f64 {
        self.scalar_fields
            .iter()
            .map(ClimateScalarAgreement::correlation)
            .fold(1.0_f64, f64::min)
    }

    /// Worst named absolute area-weighted air/SST bias.
    pub fn scalar_absolute_bias(&self) -> f64 {
        self.scalar_fields
            .iter()
            .map(ClimateScalarAgreement::absolute_area_weighted_bias)
            .fold(0.0_f64, f64::max)
    }

    pub fn vector_fields(&self) -> &[ClimateVectorAgreement] {
        &self.vector_fields
    }

    pub fn scalar_fields(&self) -> &[ClimateScalarAgreement] {
        &self.scalar_fields
    }

    pub const fn precipitation(&self) -> Option<&ClimatePrecipitationAgreement> {
        self.precipitation.as_ref()
    }

    pub fn failures(&self) -> &[ClimateAgreementFailure] {
        &self.failures
    }

    fn attach_precipitation(
        mut self,
        precipitation: ClimatePrecipitationAgreement,
        thresholds: ClimateAgreementThresholds,
    ) -> Self {
        if precipitation.correlation < thresholds.minimum_precipitation_correlation {
            self.failures.push(ClimateAgreementFailure {
                field: "precipitation",
                metric: "precipitation_correlation",
                found: precipitation.correlation,
                required: thresholds.minimum_precipitation_correlation,
            });
        }
        self.precipitation = Some(precipitation);
        self
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

    let mut vector_fields = Vec::new();
    let mut scalar_fields = Vec::new();
    let mut failures = Vec::new();
    for role in reference.active_roles() {
        let field = vector_field_name(*role);
        let metrics = weighted_vector_agreement(
            grid,
            reference.velocity_m_s(*role).expect("active role"),
            candidate.velocity_m_s(*role).expect("active role"),
        );
        if metrics.correlation < thresholds.minimum_vector_correlation {
            failures.push(ClimateAgreementFailure {
                field,
                metric: "vector_correlation",
                found: metrics.correlation,
                required: thresholds.minimum_vector_correlation,
            });
        }
        if metrics.normalized_rmse > thresholds.maximum_vector_normalized_rmse {
            failures.push(ClimateAgreementFailure {
                field,
                metric: "vector_normalized_rmse",
                found: metrics.normalized_rmse,
                required: thresholds.maximum_vector_normalized_rmse,
            });
        }
        vector_fields.push(ClimateVectorAgreement { field, ..metrics });
    }

    for (field, role) in [
        ("air_temperature", ClimateLayerRole::LowerAtmosphere),
        ("sea_surface_temperature", ClimateLayerRole::OceanMixedLayer),
    ] {
        let metrics = weighted_scalar_agreement(
            grid,
            reference.temperature_c(role).expect("locked active role"),
            candidate.temperature_c(role).expect("locked active role"),
        );
        if metrics.correlation < thresholds.minimum_scalar_correlation {
            failures.push(ClimateAgreementFailure {
                field,
                metric: "scalar_correlation",
                found: metrics.correlation,
                required: thresholds.minimum_scalar_correlation,
            });
        }
        if metrics.absolute_area_weighted_bias > thresholds.maximum_scalar_absolute_bias {
            failures.push(ClimateAgreementFailure {
                field,
                metric: "scalar_absolute_bias",
                found: metrics.absolute_area_weighted_bias,
                required: thresholds.maximum_scalar_absolute_bias,
            });
        }
        scalar_fields.push(ClimateScalarAgreement { field, ..metrics });
    }

    Ok(ClimateStateComparison {
        vector_fields,
        scalar_fields,
        precipitation: None,
        failures,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ProductionCandidateSelection {
    Selected(ProductionIntegratorId),
    NoQualifiedCandidate,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntegratorComparisonReport {
    month: usize,
    reference_steps: u32,
    imex: CandidateIntegratorComparison,
    split_explicit: CandidateIntegratorComparison,
    selection: ProductionCandidateSelection,
}

impl IntegratorComparisonReport {
    pub const fn month(&self) -> usize {
        self.month
    }

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
    // Endpoint-style transport and phase change must execute once per physical
    // step, not once per RK stage. The reference therefore refines the slow
    // process step while retaining the same equation decomposition.
    let reference_integrator = SplitExplicitRk3Integrator::new(grid, reference_step_seconds)?;
    let mut reference = initial.clone();
    let mut reference_precipitation_integral = vec![0.0_f64; grid.cell_count()];
    for _ in 0..reference_steps {
        let result = reference_integrator.advance(
            &reference,
            forcing,
            ocean_edge_permeability,
            month,
            exact_reference_step,
            cancellation,
        )?;
        for (integral, rate) in reference_precipitation_integral
            .iter_mut()
            .zip(result.mean_precipitation_rate_mm_s())
        {
            *integral += exact_reference_step * f64::from(*rate);
        }
        reference = result.into_state();
    }
    let reference_precipitation = reference_precipitation_integral
        .into_iter()
        .map(|integral| (integral / horizon_seconds) as f32)
        .collect::<Vec<_>>();
    let imex_integrator = ImexCrankNicolsonIntegrator::new(grid, 32, 1.0e-6)?;
    let imex_attempt = advance_candidate_over_product_steps(
        grid,
        initial,
        horizon_seconds,
        |state, step_seconds| {
            imex_integrator.advance(
                state,
                forcing,
                ocean_edge_permeability,
                month,
                step_seconds,
                cancellation,
            )
        },
    );
    let split_integrator = SplitExplicitRk3Integrator::new(grid, reference_step_seconds)?;
    let split_attempt = advance_candidate_over_product_steps(
        grid,
        initial,
        horizon_seconds,
        |state, step_seconds| {
            split_integrator.advance(
                state,
                forcing,
                ocean_edge_permeability,
                month,
                step_seconds,
                cancellation,
            )
        },
    );
    let thresholds = ClimateAgreementThresholds::LOCKED;
    let context = CandidateContext {
        grid,
        reference: &reference,
        reference_precipitation: &reference_precipitation,
    };
    let imex = candidate_comparison(&context, imex_attempt, thresholds)?;
    let split_explicit = candidate_comparison(&context, split_attempt, thresholds)?;
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
        month,
        reference_steps,
        imex,
        split_explicit,
        selection,
    })
}

fn advance_candidate_over_product_steps<F>(
    grid: &CubedSphereGrid,
    initial: &LayeredClimateState,
    horizon_seconds: f64,
    mut advance: F,
) -> Result<ClimateStepResult, ClimateIntegratorError>
where
    F: FnMut(&LayeredClimateState, f64) -> Result<ClimateStepResult, ClimateIntegratorError>,
{
    let step_count = (horizon_seconds / PRODUCTION_SLOW_STEP_SECONDS)
        .ceil()
        .max(1.0) as u32;
    let step_seconds = horizon_seconds / f64::from(step_count);
    let mut state = initial.clone();
    let mut diagnostics = ClimateIntegratorDiagnostics::default();
    let mut precipitation_integral = vec![0.0_f64; grid.cell_count()];
    for _ in 0..step_count {
        let result = advance(&state, step_seconds)?;
        diagnostics.accumulate(result.diagnostics());
        for (integral, rate) in precipitation_integral
            .iter_mut()
            .zip(result.mean_precipitation_rate_mm_s())
        {
            *integral += step_seconds * f64::from(*rate);
        }
        state = result.into_state();
    }
    let mean_precipitation = precipitation_integral
        .into_iter()
        .map(|integral| (integral / horizon_seconds) as f32)
        .collect();
    Ok(ClimateStepResult::new(
        state,
        diagnostics,
        mean_precipitation,
    ))
}

struct CandidateContext<'a> {
    grid: &'a CubedSphereGrid,
    reference: &'a LayeredClimateState,
    reference_precipitation: &'a [f32],
}

fn candidate_comparison(
    context: &CandidateContext<'_>,
    attempt: Result<super::ClimateStepResult, ClimateIntegratorError>,
    thresholds: ClimateAgreementThresholds,
) -> Result<CandidateIntegratorComparison, ClimateIntegratorError> {
    match attempt {
        Ok(result) => {
            let precipitation = weighted_precipitation_agreement(
                context.grid,
                context.reference_precipitation,
                result.mean_precipitation_rate_mm_s(),
            );
            let agreement = compare_climate_states(
                context.grid,
                context.reference,
                result.state(),
                thresholds,
            )?
            .attach_precipitation(precipitation, thresholds);
            Ok(CandidateIntegratorComparison {
                agreement: Some(agreement),
                diagnostics: result.diagnostics(),
                integration_failure: None,
            })
        }
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

fn vector_field_name(role: ClimateLayerRole) -> &'static str {
    match role {
        ClimateLayerRole::LowerAtmosphere => "lower_atmosphere_wind",
        ClimateLayerRole::UpperAtmosphere => "upper_atmosphere_wind",
        ClimateLayerRole::OceanMixedLayer => "ocean_mixed_layer_current",
        ClimateLayerRole::OceanThermocline => "ocean_thermocline_current",
        ClimateLayerRole::DeepOceanReservoir => unreachable!("reservoir has no velocity"),
    }
}

fn weighted_vector_agreement(
    grid: &CubedSphereGrid,
    reference: &[[f32; 3]],
    candidate: &[[f32; 3]],
) -> ClimateVectorAgreement {
    if reference == candidate {
        return ClimateVectorAgreement {
            field: "",
            correlation: 1.0,
            normalized_rmse: 0.0,
        };
    }
    let mut dot = 0.0_f64;
    let mut reference_energy = 0.0_f64;
    let mut candidate_energy = 0.0_f64;
    let mut squared_error = 0.0_f64;
    for ((cell, reference), candidate) in grid.cells().iter().zip(reference).zip(candidate) {
        let weight = cell.area_m2();
        for component in 0..3 {
            let reference = f64::from(reference[component]);
            let candidate = f64::from(candidate[component]);
            dot += weight * reference * candidate;
            reference_energy += weight * reference * reference;
            candidate_energy += weight * candidate * candidate;
            squared_error += weight * (candidate - reference).powi(2);
        }
    }
    ClimateVectorAgreement {
        field: "",
        correlation: safe_cosine(dot, reference_energy, candidate_energy),
        normalized_rmse: safe_normalized_rmse(squared_error, reference_energy),
    }
}

fn weighted_scalar_agreement(
    grid: &CubedSphereGrid,
    reference: &[f32],
    candidate: &[f32],
) -> ClimateScalarAgreement {
    if reference == candidate {
        return ClimateScalarAgreement {
            field: "",
            correlation: 1.0,
            absolute_area_weighted_bias: 0.0,
        };
    }
    let total_weight = grid.cells().iter().map(|cell| cell.area_m2()).sum::<f64>();
    let reference_mean = grid
        .cells()
        .iter()
        .zip(reference)
        .map(|(cell, value)| cell.area_m2() * f64::from(*value))
        .sum::<f64>()
        / total_weight;
    let candidate_mean = grid
        .cells()
        .iter()
        .zip(candidate)
        .map(|(cell, value)| cell.area_m2() * f64::from(*value))
        .sum::<f64>()
        / total_weight;
    let mut covariance = 0.0_f64;
    let mut reference_variance = 0.0_f64;
    let mut candidate_variance = 0.0_f64;
    for ((cell, reference), candidate) in grid.cells().iter().zip(reference).zip(candidate) {
        let weight = cell.area_m2();
        let reference = f64::from(*reference) - reference_mean;
        let candidate = f64::from(*candidate) - candidate_mean;
        covariance += weight * reference * candidate;
        reference_variance += weight * reference * reference;
        candidate_variance += weight * candidate * candidate;
    }
    ClimateScalarAgreement {
        field: "",
        correlation: safe_cosine(covariance, reference_variance, candidate_variance),
        absolute_area_weighted_bias: (candidate_mean - reference_mean).abs(),
    }
}

fn weighted_precipitation_agreement(
    grid: &CubedSphereGrid,
    reference: &[f32],
    candidate: &[f32],
) -> ClimatePrecipitationAgreement {
    let scalar = weighted_scalar_agreement(grid, reference, candidate);
    let reference_total = grid
        .cells()
        .iter()
        .zip(reference)
        .map(|(cell, value)| cell.area_m2() * f64::from(*value))
        .sum::<f64>();
    let candidate_total = grid
        .cells()
        .iter()
        .zip(candidate)
        .map(|(cell, value)| cell.area_m2() * f64::from(*value))
        .sum::<f64>();
    ClimatePrecipitationAgreement {
        correlation: scalar.correlation,
        reference_area_integrated_rate: reference_total,
        candidate_area_integrated_rate: candidate_total,
    }
}

/// Computes the annual area-integrated precipitation bias over exactly one
/// complete 12-month report set. Reports may be in any order but each month
/// must occur once.
pub fn annual_precipitation_total_bias(
    reports: &[IntegratorComparisonReport],
    candidate: ProductionIntegratorId,
) -> Option<f64> {
    if reports.len() != 12 {
        return None;
    }
    let mut seen = [false; 12];
    let mut reference_total = 0.0_f64;
    let mut candidate_total = 0.0_f64;
    for report in reports {
        if report.month >= 12 || seen[report.month] {
            return None;
        }
        seen[report.month] = true;
        let candidate = match candidate {
            ProductionIntegratorId::ImexCrankNicolsonV1 => report.imex(),
            ProductionIntegratorId::SplitExplicitRk3V1 => report.split_explicit(),
        };
        let precipitation = candidate.agreement()?.precipitation()?;
        reference_total += precipitation.reference_area_integrated_rate;
        candidate_total += precipitation.candidate_area_integrated_rate;
    }
    seen.into_iter()
        .all(|seen| seen)
        .then(|| relative_total_bias(reference_total, candidate_total))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ClimateConservationInterpretation {
    SharedTendencyExtensiveV1,
    /// A deliberately distinct procedure identity for implementations that
    /// infer conservation from private state deltas instead of the shared
    /// extensive tendency ledger. Production P4 integrators do not use it;
    /// retaining it makes the identity gate falsifiable.
    IntegratorInternalStateDeltaV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FormationProcedureIdentity {
    capabilities: ClimateCapabilitySet,
    conservation_interpretation: ClimateConservationInterpretation,
    model_fingerprint: [u8; 32],
}

impl FormationProcedureIdentity {
    pub fn new(
        capabilities: ClimateCapabilitySet,
        conservation_interpretation: ClimateConservationInterpretation,
        model_fingerprint: [u8; 32],
    ) -> Self {
        Self {
            capabilities,
            conservation_interpretation,
            model_fingerprint,
        }
    }

    pub const fn capabilities(&self) -> &ClimateCapabilitySet {
        &self.capabilities
    }

    pub const fn conservation_interpretation(&self) -> ClimateConservationInterpretation {
        self.conservation_interpretation
    }

    pub const fn model_fingerprint(&self) -> &[u8; 32] {
        &self.model_fingerprint
    }
}

/// Checks the independently declared scientific procedure identities used by
/// two formation runs.
pub fn formation_procedure_identity_matches(
    reference: &FormationProcedureIdentity,
    candidate: &FormationProcedureIdentity,
) -> bool {
    candidate == reference
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormationProcedureAgreement {
    imex_capability_set_match: bool,
    split_explicit_capability_set_match: bool,
    imex_conservation_interpretation_match: bool,
    split_explicit_conservation_interpretation_match: bool,
    imex_model_fingerprint_match: bool,
    split_explicit_model_fingerprint_match: bool,
}

impl FormationProcedureAgreement {
    pub const fn imex_capability_set_match(self) -> bool {
        self.imex_capability_set_match
    }

    pub const fn split_explicit_capability_set_match(self) -> bool {
        self.split_explicit_capability_set_match
    }

    pub const fn imex_conservation_interpretation_match(self) -> bool {
        self.imex_conservation_interpretation_match
    }

    pub const fn split_explicit_conservation_interpretation_match(self) -> bool {
        self.split_explicit_conservation_interpretation_match
    }

    pub const fn capability_set_match(self) -> bool {
        self.imex_capability_set_match && self.split_explicit_capability_set_match
    }

    pub const fn conservation_interpretation_match(self) -> bool {
        self.imex_conservation_interpretation_match
            && self.split_explicit_conservation_interpretation_match
    }

    pub const fn imex_model_fingerprint_match(self) -> bool {
        self.imex_model_fingerprint_match
    }

    pub const fn split_explicit_model_fingerprint_match(self) -> bool {
        self.split_explicit_model_fingerprint_match
    }

    pub const fn model_fingerprint_match(self) -> bool {
        self.imex_model_fingerprint_match && self.split_explicit_model_fingerprint_match
    }

    pub const fn qualifies(self) -> bool {
        self.capability_set_match()
            && self.conservation_interpretation_match()
            && self.model_fingerprint_match()
    }
}

/// Uses the same three-way gate as the serialized formation report. Tests can
/// inject a mismatched identity here and prove the complete aggregate gate is
/// not a self-comparison.
pub fn compare_formation_procedure_identities(
    reference: &FormationProcedureIdentity,
    imex: &FormationProcedureIdentity,
    split_explicit: &FormationProcedureIdentity,
) -> FormationProcedureAgreement {
    FormationProcedureAgreement {
        imex_capability_set_match: imex.capabilities == reference.capabilities,
        split_explicit_capability_set_match: split_explicit.capabilities == reference.capabilities,
        imex_conservation_interpretation_match: imex.conservation_interpretation
            == reference.conservation_interpretation,
        split_explicit_conservation_interpretation_match: split_explicit
            .conservation_interpretation
            == reference.conservation_interpretation,
        imex_model_fingerprint_match: imex.model_fingerprint == reference.model_fingerprint,
        split_explicit_model_fingerprint_match: split_explicit.model_fingerprint
            == reference.model_fingerprint,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FormationRunOutcome {
    cycles: Option<u16>,
    final_residual: Option<f64>,
    failure: Option<String>,
    procedure: FormationProcedureIdentity,
}

impl FormationRunOutcome {
    pub const fn cycles(&self) -> Option<u16> {
        self.cycles
    }

    pub const fn final_residual(&self) -> Option<f64> {
        self.final_residual
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub const fn procedure(&self) -> &FormationProcedureIdentity {
        &self.procedure
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FormationCycleComparisonReport {
    reference: FormationRunOutcome,
    imex: FormationRunOutcome,
    split_explicit: FormationRunOutcome,
    imex_cycle_match: bool,
    split_explicit_cycle_match: bool,
    imex_capability_set_match: bool,
    split_explicit_capability_set_match: bool,
    capability_set_match: bool,
    imex_conservation_interpretation_match: bool,
    split_explicit_conservation_interpretation_match: bool,
    conservation_interpretation_match: bool,
    conservation_interpretation: ClimateConservationInterpretation,
    imex_model_fingerprint_match: bool,
    split_explicit_model_fingerprint_match: bool,
    model_fingerprint_match: bool,
}

impl FormationCycleComparisonReport {
    pub const fn reference(&self) -> &FormationRunOutcome {
        &self.reference
    }

    pub const fn imex(&self) -> &FormationRunOutcome {
        &self.imex
    }

    pub const fn split_explicit(&self) -> &FormationRunOutcome {
        &self.split_explicit
    }

    pub const fn imex_cycle_match(&self) -> bool {
        self.imex_cycle_match
    }

    pub const fn split_explicit_cycle_match(&self) -> bool {
        self.split_explicit_cycle_match
    }

    pub const fn capability_set_match(&self) -> bool {
        self.capability_set_match
    }

    pub const fn imex_capability_set_match(&self) -> bool {
        self.imex_capability_set_match
    }

    pub const fn split_explicit_capability_set_match(&self) -> bool {
        self.split_explicit_capability_set_match
    }

    pub const fn conservation_interpretation_match(&self) -> bool {
        self.conservation_interpretation_match
    }

    pub const fn imex_conservation_interpretation_match(&self) -> bool {
        self.imex_conservation_interpretation_match
    }

    pub const fn split_explicit_conservation_interpretation_match(&self) -> bool {
        self.split_explicit_conservation_interpretation_match
    }

    pub const fn imex_model_fingerprint_match(&self) -> bool {
        self.imex_model_fingerprint_match
    }

    pub const fn split_explicit_model_fingerprint_match(&self) -> bool {
        self.split_explicit_model_fingerprint_match
    }

    pub const fn model_fingerprint_match(&self) -> bool {
        self.model_fingerprint_match
    }
}

#[derive(Debug, Clone, Copy)]
enum FormationIntegrator {
    RefinedSplitReference,
    Imex,
    SplitExplicit,
}

/// Runs the locked January-to-December formation procedure independently for
/// a process-consistent refined reference and both candidates, then checks
/// exact stopping-cycle identity. All three paths use the same capability
/// inventory and extensive source/sink interpretation.
#[allow(clippy::too_many_arguments)]
pub fn run_formation_cycle_comparison(
    grid: &CubedSphereGrid,
    initial: &LayeredClimateState,
    forcing: &PlanetForcing,
    ocean_edge_permeability: &[f32],
    maximum_cycles: u16,
    macro_step_seconds: f64,
    reference_step_seconds: f64,
    cancellation: &BuildCancellation,
) -> Result<FormationCycleComparisonReport, ClimateIntegratorError> {
    if maximum_cycles == 0 {
        return Err(ClimateIntegratorError::InvalidTimeStep { found: 0.0 });
    }
    if !macro_step_seconds.is_finite() || macro_step_seconds <= 0.0 {
        return Err(ClimateIntegratorError::InvalidTimeStep {
            found: macro_step_seconds,
        });
    }
    if !reference_step_seconds.is_finite() || reference_step_seconds <= 0.0 {
        return Err(ClimateIntegratorError::InvalidFastStep {
            found: reference_step_seconds,
        });
    }
    let reference = run_formation(
        FormationIntegrator::RefinedSplitReference,
        grid,
        initial,
        forcing,
        ocean_edge_permeability,
        maximum_cycles,
        macro_step_seconds,
        reference_step_seconds,
        cancellation,
    )?;
    let imex = run_formation(
        FormationIntegrator::Imex,
        grid,
        initial,
        forcing,
        ocean_edge_permeability,
        maximum_cycles,
        macro_step_seconds,
        reference_step_seconds,
        cancellation,
    )?;
    let split_explicit = run_formation(
        FormationIntegrator::SplitExplicit,
        grid,
        initial,
        forcing,
        ocean_edge_permeability,
        maximum_cycles,
        macro_step_seconds,
        reference_step_seconds,
        cancellation,
    )?;
    let procedure_agreement = compare_formation_procedure_identities(
        &reference.procedure,
        &imex.procedure,
        &split_explicit.procedure,
    );
    let interpretation = reference.procedure.conservation_interpretation;
    Ok(FormationCycleComparisonReport {
        imex_cycle_match: imex.cycles.is_some() && imex.cycles == reference.cycles,
        split_explicit_cycle_match: split_explicit.cycles.is_some()
            && split_explicit.cycles == reference.cycles,
        reference,
        imex,
        split_explicit,
        imex_capability_set_match: procedure_agreement.imex_capability_set_match(),
        split_explicit_capability_set_match: procedure_agreement
            .split_explicit_capability_set_match(),
        capability_set_match: procedure_agreement.capability_set_match(),
        imex_conservation_interpretation_match: procedure_agreement
            .imex_conservation_interpretation_match(),
        split_explicit_conservation_interpretation_match: procedure_agreement
            .split_explicit_conservation_interpretation_match(),
        conservation_interpretation_match: procedure_agreement.conservation_interpretation_match(),
        conservation_interpretation: interpretation,
        imex_model_fingerprint_match: procedure_agreement.imex_model_fingerprint_match(),
        split_explicit_model_fingerprint_match: procedure_agreement
            .split_explicit_model_fingerprint_match(),
        model_fingerprint_match: procedure_agreement.model_fingerprint_match(),
    })
}

#[allow(clippy::too_many_arguments)]
fn run_formation(
    integrator: FormationIntegrator,
    grid: &CubedSphereGrid,
    initial: &LayeredClimateState,
    forcing: &PlanetForcing,
    ocean_edge_permeability: &[f32],
    maximum_cycles: u16,
    macro_step_seconds: f64,
    reference_step_seconds: f64,
    cancellation: &BuildCancellation,
) -> Result<FormationRunOutcome, ClimateIntegratorError> {
    let mut state = initial.clone();
    let procedure = actual_integrator_procedure_identity(
        integrator,
        grid,
        initial.profile(),
        reference_step_seconds,
    )?;
    let mut previous_annual = state.clone();
    let mut final_residual = None;
    for cycle in 1..=maximum_cycles {
        for month in 0..CLIMATE_MONTH_COUNT {
            let attempt = advance_formation_month(
                integrator,
                grid,
                &state,
                forcing,
                ocean_edge_permeability,
                month,
                macro_step_seconds,
                reference_step_seconds,
                cancellation,
            );
            state = match attempt {
                Ok(state) => state,
                Err(ClimateIntegratorError::Cancelled) => {
                    return Err(ClimateIntegratorError::Cancelled)
                }
                Err(error) if !matches!(integrator, FormationIntegrator::RefinedSplitReference) => {
                    return Ok(FormationRunOutcome {
                        cycles: None,
                        final_residual,
                        failure: Some(error.to_string()),
                        procedure: procedure.clone(),
                    });
                }
                Err(error) => return Err(error),
            };
            state.enforce_full_land_ocean_velocity(forcing, cancellation)?;
        }
        let residual = formation_residual(grid, &previous_annual, &state)?;
        final_residual = Some(residual);
        if residual <= FORMATION_COMPARISON_RESIDUAL_TARGET {
            return Ok(FormationRunOutcome {
                cycles: Some(cycle),
                final_residual,
                failure: None,
                procedure: procedure.clone(),
            });
        }
        previous_annual = state.clone();
    }
    Ok(FormationRunOutcome {
        cycles: None,
        final_residual,
        failure: Some(format!(
            "formation did not converge within {maximum_cycles} cycles"
        )),
        procedure,
    })
}

fn actual_integrator_procedure_identity(
    integrator: FormationIntegrator,
    grid: &CubedSphereGrid,
    profile: ClimateModelProfile,
    reference_step_seconds: f64,
) -> Result<FormationProcedureIdentity, ClimateIntegratorError> {
    match integrator {
        FormationIntegrator::RefinedSplitReference => Ok(SplitExplicitRk3Integrator::new(
            grid,
            reference_step_seconds,
        )?
        .formation_procedure_identity(profile)),
        FormationIntegrator::Imex => Ok(ImexCrankNicolsonIntegrator::new(grid, 32, 1.0e-6)?
            .formation_procedure_identity(profile)),
        FormationIntegrator::SplitExplicit => Ok(SplitExplicitRk3Integrator::new(
            grid,
            reference_step_seconds,
        )?
        .formation_procedure_identity(profile)),
    }
}

#[allow(clippy::too_many_arguments)]
fn advance_formation_month(
    integrator: FormationIntegrator,
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    forcing: &PlanetForcing,
    ocean_edge_permeability: &[f32],
    month: usize,
    macro_step_seconds: f64,
    reference_step_seconds: f64,
    cancellation: &BuildCancellation,
) -> Result<LayeredClimateState, ClimateIntegratorError> {
    match integrator {
        FormationIntegrator::RefinedSplitReference => {
            let steps = (macro_step_seconds / reference_step_seconds)
                .ceil()
                .max(1.0) as u32;
            let step_seconds = macro_step_seconds / f64::from(steps);
            let integrator = SplitExplicitRk3Integrator::new(grid, reference_step_seconds)?;
            let mut state = state.clone();
            for _ in 0..steps {
                state = integrator
                    .advance(
                        &state,
                        forcing,
                        ocean_edge_permeability,
                        month,
                        step_seconds,
                        cancellation,
                    )?
                    .into_state();
            }
            Ok(state)
        }
        FormationIntegrator::Imex => Ok(ImexCrankNicolsonIntegrator::new(grid, 32, 1.0e-6)?
            .advance(
                state,
                forcing,
                ocean_edge_permeability,
                month,
                macro_step_seconds,
                cancellation,
            )?
            .into_state()),
        FormationIntegrator::SplitExplicit => Ok(SplitExplicitRk3Integrator::new(
            grid,
            reference_step_seconds,
        )?
        .advance(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            macro_step_seconds,
            cancellation,
        )?
        .into_state()),
    }
}

fn formation_residual(
    grid: &CubedSphereGrid,
    previous: &LayeredClimateState,
    current: &LayeredClimateState,
) -> Result<f64, ClimateIntegratorError> {
    super::climate_state_formation_residual(grid, previous, current)
}

fn relative_total_bias(reference: f64, candidate: f64) -> f64 {
    if reference.abs() > f64::EPSILON {
        (candidate - reference).abs() / reference.abs()
    } else if candidate == reference {
        0.0
    } else {
        f64::INFINITY
    }
}

fn safe_cosine(dot: f64, left_energy: f64, right_energy: f64) -> f64 {
    let denominator = (left_energy * right_energy).sqrt();
    if denominator == 0.0 {
        0.0
    } else {
        (dot / denominator).clamp(-1.0, 1.0)
    }
}

fn safe_normalized_rmse(squared_error: f64, reference_energy: f64) -> f64 {
    if squared_error == 0.0 {
        0.0
    } else if reference_energy == 0.0 {
        f64::INFINITY
    } else {
        (squared_error / reference_energy).sqrt()
    }
}
