use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{MonthlyScalarField, MonthlyVector3Field, NaturalQualityProfile, CLIMATE_MONTH_COUNT};
use crate::world::spatial::{
    ConservativeSurfaceMap, SphericalSurfaceSnapshot, SurfaceGeometryKind, SurfaceRef,
};
use crate::world::CellId;

/// The first strict schema for the reconstructable climate work domain.
pub const CLIMATE_WORK_DOMAIN_SCHEMA_V1: u16 = 1;
/// The first public layered atmosphere-ocean climatology schema.
pub const GLOBAL_CIRCULATION_SCHEMA_V1: u16 = 1;
/// The first fixed-layout schema.
pub const CLIMATE_LAYER_LAYOUT_SCHEMA_V1: u16 = 1;
/// The first resumable climate-checkpoint identity schema.
pub const CLIMATE_CHECKPOINT_SCHEMA_V1: u16 = 1;
/// Maximum accepted radial component after publishing an `f32` tangent vector.
pub const GLOBAL_CIRCULATION_TANGENCY_TOLERANCE_M_S: f64 = 1.0e-4;
/// Maximum solver-reported relative mass, volume, moisture, or exchange error.
pub const GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX: f64 = 1.0e-6;
/// Energy integrates more source terms and uses a separately declared bound.
pub const GLOBAL_CIRCULATION_ENERGY_RELATIVE_ERROR_MAX: f64 = 1.0e-5;

/// Closed scientific layer configurations supported by the climate core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClimateModelProfile {
    C1SingleLayerV1,
    C2LayeredV1,
}

/// Stable semantic roles; numerical layer indices never escape the solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClimateLayerRole {
    LowerAtmosphere,
    UpperAtmosphere,
    OceanMixedLayer,
    OceanThermocline,
    DeepOceanReservoir,
}

/// One immutable member of a fixed climate profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateLayerSpec {
    role: ClimateLayerRole,
    dynamically_active: bool,
    reference_thickness_m: f64,
    density_kg_m3: f64,
    heat_capacity_j_kg_k: f64,
    exchange_time_s: f64,
}

impl ClimateLayerSpec {
    pub const fn role(&self) -> ClimateLayerRole {
        self.role
    }

    pub const fn dynamically_active(&self) -> bool {
        self.dynamically_active
    }

    pub const fn reference_thickness_m(&self) -> f64 {
        self.reference_thickness_m
    }

    pub const fn density_kg_m3(&self) -> f64 {
        self.density_kg_m3
    }

    pub const fn heat_capacity_j_kg_k(&self) -> f64 {
        self.heat_capacity_j_kg_k
    }

    pub const fn exchange_time_s(&self) -> f64 {
        self.exchange_time_s
    }
}

/// The exact layer inventory and declared physical reference constants.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateLayerLayout {
    schema_version: u16,
    profile: ClimateModelProfile,
    layers: Vec<ClimateLayerSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateLayerLayoutWire {
    schema_version: u16,
    profile: ClimateModelProfile,
    layers: Vec<ClimateLayerSpec>,
}

impl ClimateLayerLayout {
    /// Returns the only legal layout for a closed model profile.
    pub fn for_profile(profile: ClimateModelProfile) -> Self {
        let atmosphere = |role, thickness, exchange_days| ClimateLayerSpec {
            role,
            dynamically_active: true,
            reference_thickness_m: thickness,
            density_kg_m3: 1.225,
            heat_capacity_j_kg_k: 1_004.0,
            exchange_time_s: exchange_days * 86_400.0,
        };
        let ocean = |role, thickness, exchange_days, active| ClimateLayerSpec {
            role,
            dynamically_active: active,
            reference_thickness_m: thickness,
            density_kg_m3: 1_025.0,
            heat_capacity_j_kg_k: 3_990.0,
            exchange_time_s: exchange_days * 86_400.0,
        };
        let layers = match profile {
            ClimateModelProfile::C1SingleLayerV1 => vec![
                atmosphere(ClimateLayerRole::LowerAtmosphere, 8_000.0, 5.0),
                ocean(ClimateLayerRole::OceanMixedLayer, 100.0, 90.0, true),
            ],
            ClimateModelProfile::C2LayeredV1 => vec![
                atmosphere(ClimateLayerRole::LowerAtmosphere, 6_000.0, 5.0),
                atmosphere(ClimateLayerRole::UpperAtmosphere, 4_000.0, 10.0),
                ocean(ClimateLayerRole::OceanMixedLayer, 100.0, 90.0, true),
                ocean(
                    ClimateLayerRole::OceanThermocline,
                    900.0,
                    365.25 * 5.0,
                    true,
                ),
                ocean(
                    ClimateLayerRole::DeepOceanReservoir,
                    3_000.0,
                    365.25 * 200.0,
                    false,
                ),
            ],
        };
        Self {
            schema_version: CLIMATE_LAYER_LAYOUT_SCHEMA_V1,
            profile,
            layers,
        }
    }

    pub fn validate(&self) -> Result<(), ClimateLayerLayoutError> {
        if self.schema_version != CLIMATE_LAYER_LAYOUT_SCHEMA_V1 {
            return Err(ClimateLayerLayoutError::UnsupportedSchema {
                found: self.schema_version,
                supported: CLIMATE_LAYER_LAYOUT_SCHEMA_V1,
            });
        }
        let expected = Self::for_profile(self.profile);
        if self.layers != expected.layers {
            return Err(ClimateLayerLayoutError::ProfileDefinitionMismatch {
                profile: self.profile,
            });
        }
        Ok(())
    }

    pub const fn profile(&self) -> ClimateModelProfile {
        self.profile
    }

    pub fn layers(&self) -> &[ClimateLayerSpec] {
        &self.layers
    }

    /// Fingerprints the scientific layer definition independently of serde.
    pub fn fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.climate-layer-layout.v1\0");
        hasher.update(&self.schema_version.to_le_bytes());
        hasher.update(&[model_profile_tag(self.profile)]);
        hasher.update(&(self.layers.len() as u32).to_le_bytes());
        for layer in &self.layers {
            hasher.update(&[layer_role_tag(layer.role)]);
            hasher.update(&[u8::from(layer.dynamically_active)]);
            for value in [
                layer.reference_thickness_m,
                layer.density_kg_m3,
                layer.heat_capacity_j_kg_k,
                layer.exchange_time_s,
            ] {
                hasher.update(&value.to_bits().to_le_bytes());
            }
        }
        *hasher.finalize().as_bytes()
    }
}

impl<'de> Deserialize<'de> for ClimateLayerLayout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateLayerLayoutWire::deserialize(deserializer)?;
        let layout = Self {
            schema_version: wire.schema_version,
            profile: wire.profile,
            layers: wire.layers,
        };
        layout.validate().map_err(D::Error::custom)?;
        Ok(layout)
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateLayerLayoutError {
    #[error("unsupported climate layer-layout schema {found}; supported schema is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("serialized layers do not equal the fixed {profile:?} definition")]
    ProfileDefinitionMismatch { profile: ClimateModelProfile },
}

/// Product integrators that may own a published P4 snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionIntegratorId {
    ImexCrankNicolsonV1,
    SplitExplicitRk3V1,
}

/// Stable floating-point and reduction protocol used by resumable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClimateQuantizationId {
    DeterministicF64V1,
}

/// Capability IDs whose absence must never be inferred from a missing field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClimateCapabilityId {
    SeasonalMeanV1,
    VerticalStructureV1,
    SeaIceV1,
    LandSurfaceFeedbackV1,
    EquatorialVariabilityV1,
    TropicalCycloneClimatologyV1,
}

const ALL_CLIMATE_CAPABILITIES: [ClimateCapabilityId; 6] = [
    ClimateCapabilityId::SeasonalMeanV1,
    ClimateCapabilityId::VerticalStructureV1,
    ClimateCapabilityId::SeaIceV1,
    ClimateCapabilityId::LandSurfaceFeedbackV1,
    ClimateCapabilityId::EquatorialVariabilityV1,
    ClimateCapabilityId::TropicalCycloneClimatologyV1,
];

/// Explicit three-state capability outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClimateCapabilityAvailability {
    Unavailable,
    EvaluatedNotApplicable,
    Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateCapabilityStatus {
    id: ClimateCapabilityId,
    availability: ClimateCapabilityAvailability,
}

/// A complete, canonical inventory of all known climate capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateCapabilitySet {
    statuses: Vec<ClimateCapabilityStatus>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateCapabilitySetWire {
    statuses: Vec<ClimateCapabilityStatus>,
}

impl ClimateCapabilitySet {
    pub fn new(
        statuses: Vec<(ClimateCapabilityId, ClimateCapabilityAvailability)>,
    ) -> Result<Self, ClimateCapabilityError> {
        let mut statuses = statuses
            .into_iter()
            .map(|(id, availability)| ClimateCapabilityStatus { id, availability })
            .collect::<Vec<_>>();
        statuses.sort_by_key(|status| status.id);
        let set = Self { statuses };
        set.validate()?;
        Ok(set)
    }

    pub fn for_profile(profile: ClimateModelProfile) -> Self {
        Self::new(
            ALL_CLIMATE_CAPABILITIES
                .into_iter()
                .map(|id| {
                    let availability = match id {
                        ClimateCapabilityId::SeasonalMeanV1 => {
                            ClimateCapabilityAvailability::Available
                        }
                        ClimateCapabilityId::VerticalStructureV1
                            if profile == ClimateModelProfile::C2LayeredV1 =>
                        {
                            ClimateCapabilityAvailability::Available
                        }
                        _ => ClimateCapabilityAvailability::Unavailable,
                    };
                    (id, availability)
                })
                .collect(),
        )
        .expect("closed profile capability inventory is valid")
    }

    pub fn validate(&self) -> Result<(), ClimateCapabilityError> {
        if self.statuses.len() != ALL_CLIMATE_CAPABILITIES.len() {
            return Err(ClimateCapabilityError::IncompleteInventory {
                found: self.statuses.len(),
                expected: ALL_CLIMATE_CAPABILITIES.len(),
            });
        }
        for (index, expected) in ALL_CLIMATE_CAPABILITIES.iter().enumerate() {
            if self.statuses[index].id != *expected {
                return Err(ClimateCapabilityError::NonCanonicalInventory { index });
            }
        }
        Ok(())
    }

    pub fn availability(&self, id: ClimateCapabilityId) -> ClimateCapabilityAvailability {
        self.statuses
            .iter()
            .find(|status| status.id == id)
            .map(|status| status.availability)
            .expect("validated capability sets contain every closed ID")
    }
}

impl<'de> Deserialize<'de> for ClimateCapabilitySet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateCapabilitySetWire::deserialize(deserializer)?;
        Self::new(
            wire.statuses
                .into_iter()
                .map(|status| (status.id, status.availability))
                .collect(),
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClimateCapabilityError {
    #[error("capability inventory has {found} entries, expected {expected}")]
    IncompleteInventory { found: usize, expected: usize },
    #[error("capability inventory is duplicate, missing, or out of canonical order at {index}")]
    NonCanonicalInventory { index: usize },
}

/// Strict identity and state hash for a resumable formation checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateCheckpoint {
    schema_version: u16,
    profile: ClimateModelProfile,
    integrator: ProductionIntegratorId,
    grid_fingerprint: [u8; 32],
    forcing_fingerprint: [u8; 32],
    model_fingerprint: [u8; 32],
    input_fingerprint: [u8; 32],
    quantization: ClimateQuantizationId,
    completed_months: u32,
    state_fingerprint: [u8; 32],
    fingerprint: [u8; 32],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateCheckpointWire {
    schema_version: u16,
    profile: ClimateModelProfile,
    integrator: ProductionIntegratorId,
    grid_fingerprint: [u8; 32],
    forcing_fingerprint: [u8; 32],
    model_fingerprint: [u8; 32],
    input_fingerprint: [u8; 32],
    quantization: ClimateQuantizationId,
    completed_months: u32,
    state_fingerprint: [u8; 32],
    fingerprint: [u8; 32],
}

impl ClimateCheckpoint {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        profile: ClimateModelProfile,
        integrator: ProductionIntegratorId,
        grid_fingerprint: [u8; 32],
        forcing_fingerprint: [u8; 32],
        model_fingerprint: [u8; 32],
        input_fingerprint: [u8; 32],
        quantization: ClimateQuantizationId,
        completed_months: u32,
        state_fingerprint: [u8; 32],
    ) -> Result<Self, ClimateCheckpointError> {
        let mut checkpoint = Self {
            schema_version: CLIMATE_CHECKPOINT_SCHEMA_V1,
            profile,
            integrator,
            grid_fingerprint,
            forcing_fingerprint,
            model_fingerprint,
            input_fingerprint,
            quantization,
            completed_months,
            state_fingerprint,
            fingerprint: [0; 32],
        };
        checkpoint.validate_identity()?;
        checkpoint.fingerprint = checkpoint.canonical_fingerprint();
        Ok(checkpoint)
    }

    pub fn validate(&self) -> Result<(), ClimateCheckpointError> {
        self.validate_identity()?;
        let calculated = self.canonical_fingerprint();
        if self.fingerprint != calculated {
            return Err(ClimateCheckpointError::FingerprintMismatch);
        }
        Ok(())
    }

    fn validate_identity(&self) -> Result<(), ClimateCheckpointError> {
        if self.schema_version != CLIMATE_CHECKPOINT_SCHEMA_V1 {
            return Err(ClimateCheckpointError::UnsupportedSchema {
                found: self.schema_version,
                supported: CLIMATE_CHECKPOINT_SCHEMA_V1,
            });
        }
        for (field, fingerprint) in [
            ("grid_fingerprint", self.grid_fingerprint),
            ("forcing_fingerprint", self.forcing_fingerprint),
            ("model_fingerprint", self.model_fingerprint),
            ("input_fingerprint", self.input_fingerprint),
            ("state_fingerprint", self.state_fingerprint),
        ] {
            if fingerprint == [0; 32] {
                return Err(ClimateCheckpointError::ZeroFingerprint { field });
            }
        }
        if self.completed_months == 0
            || self.completed_months % u32::try_from(CLIMATE_MONTH_COUNT).unwrap_or(12) != 0
        {
            return Err(ClimateCheckpointError::InvalidCompletedMonths {
                found: self.completed_months,
            });
        }
        Ok(())
    }

    fn canonical_fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.climate-checkpoint.v1\0");
        hasher.update(&self.schema_version.to_le_bytes());
        hasher.update(&[model_profile_tag(self.profile)]);
        hasher.update(&[integrator_tag(self.integrator)]);
        hasher.update(&self.grid_fingerprint);
        hasher.update(&self.forcing_fingerprint);
        hasher.update(&self.model_fingerprint);
        hasher.update(&self.input_fingerprint);
        hasher.update(&[quantization_tag(self.quantization)]);
        hasher.update(&self.completed_months.to_le_bytes());
        hasher.update(&self.state_fingerprint);
        *hasher.finalize().as_bytes()
    }

    pub const fn profile(&self) -> ClimateModelProfile {
        self.profile
    }

    pub const fn integrator(&self) -> ProductionIntegratorId {
        self.integrator
    }

    pub const fn grid_fingerprint(&self) -> &[u8; 32] {
        &self.grid_fingerprint
    }

    pub const fn forcing_fingerprint(&self) -> &[u8; 32] {
        &self.forcing_fingerprint
    }

    pub const fn model_fingerprint(&self) -> &[u8; 32] {
        &self.model_fingerprint
    }

    pub const fn input_fingerprint(&self) -> &[u8; 32] {
        &self.input_fingerprint
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

impl<'de> Deserialize<'de> for ClimateCheckpoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateCheckpointWire::deserialize(deserializer)?;
        let mut checkpoint = Self::new(
            wire.profile,
            wire.integrator,
            wire.grid_fingerprint,
            wire.forcing_fingerprint,
            wire.model_fingerprint,
            wire.input_fingerprint,
            wire.quantization,
            wire.completed_months,
            wire.state_fingerprint,
        )
        .map_err(D::Error::custom)?;
        if wire.schema_version != CLIMATE_CHECKPOINT_SCHEMA_V1 {
            return Err(D::Error::custom(
                ClimateCheckpointError::UnsupportedSchema {
                    found: wire.schema_version,
                    supported: CLIMATE_CHECKPOINT_SCHEMA_V1,
                },
            ));
        }
        if checkpoint.fingerprint != wire.fingerprint {
            return Err(D::Error::custom(
                ClimateCheckpointError::FingerprintMismatch,
            ));
        }
        checkpoint.fingerprint = wire.fingerprint;
        Ok(checkpoint)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClimateCheckpointError {
    #[error("unsupported climate checkpoint schema {found}; supported schema is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("checkpoint {field} cannot be zero")]
    ZeroFingerprint { field: &'static str },
    #[error("checkpoint completed months {found} must be a positive whole number of years")]
    InvalidCompletedMonths { found: u32 },
    #[error("climate checkpoint fingerprint does not match its semantic fields")]
    FingerprintMismatch,
}

const fn model_profile_tag(profile: ClimateModelProfile) -> u8 {
    match profile {
        ClimateModelProfile::C1SingleLayerV1 => 1,
        ClimateModelProfile::C2LayeredV1 => 2,
    }
}

const fn layer_role_tag(role: ClimateLayerRole) -> u8 {
    match role {
        ClimateLayerRole::LowerAtmosphere => 1,
        ClimateLayerRole::UpperAtmosphere => 2,
        ClimateLayerRole::OceanMixedLayer => 3,
        ClimateLayerRole::OceanThermocline => 4,
        ClimateLayerRole::DeepOceanReservoir => 5,
    }
}

const fn integrator_tag(integrator: ProductionIntegratorId) -> u8 {
    match integrator {
        ProductionIntegratorId::ImexCrankNicolsonV1 => 1,
        ProductionIntegratorId::SplitExplicitRk3V1 => 2,
    }
}

const fn quantization_tag(quantization: ClimateQuantizationId) -> u8 {
    match quantization {
        ClimateQuantizationId::DeterministicF64V1 => 1,
    }
}

/// Bounded formation and numerical-convergence evidence.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateSolveReport {
    formation_years: u16,
    macro_steps: u64,
    fast_substeps: u64,
    linear_iterations: u64,
    initial_residual: f64,
    final_residual: f64,
    maximum_cfl: f64,
    dense_state_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateSolveReportWire {
    formation_years: u16,
    macro_steps: u64,
    fast_substeps: u64,
    linear_iterations: u64,
    initial_residual: f64,
    final_residual: f64,
    maximum_cfl: f64,
    dense_state_bytes: u64,
}

impl ClimateSolveReport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        formation_years: u16,
        macro_steps: u64,
        fast_substeps: u64,
        linear_iterations: u64,
        initial_residual: f64,
        final_residual: f64,
        maximum_cfl: f64,
        dense_state_bytes: u64,
    ) -> Result<Self, ClimateReportError> {
        let report = Self {
            formation_years,
            macro_steps,
            fast_substeps,
            linear_iterations,
            initial_residual,
            final_residual,
            maximum_cfl,
            dense_state_bytes,
        };
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), ClimateReportError> {
        for (field, value) in [
            ("initial_residual", self.initial_residual),
            ("final_residual", self.final_residual),
            ("maximum_cfl", self.maximum_cfl),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(ClimateReportError::InvalidStatistic {
                    field,
                    found: value,
                });
            }
        }
        if self.formation_years == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "formation_years",
            });
        }
        if self.macro_steps == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "macro_steps",
            });
        }
        if self.fast_substeps == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "fast_substeps",
            });
        }
        if self.dense_state_bytes == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "dense_state_bytes",
            });
        }
        if self.final_residual > self.initial_residual {
            return Err(ClimateReportError::ResidualIncreased {
                initial: self.initial_residual,
                final_value: self.final_residual,
            });
        }
        if self.maximum_cfl > 1.0 {
            return Err(ClimateReportError::StatisticAboveMaximum {
                field: "maximum_cfl",
                found: self.maximum_cfl,
                maximum: 1.0,
            });
        }
        Ok(())
    }

    pub const fn formation_years(&self) -> u16 {
        self.formation_years
    }

    pub const fn macro_steps(&self) -> u64 {
        self.macro_steps
    }

    pub const fn fast_substeps(&self) -> u64 {
        self.fast_substeps
    }

    pub const fn linear_iterations(&self) -> u64 {
        self.linear_iterations
    }

    pub const fn final_residual(&self) -> f64 {
        self.final_residual
    }

    pub const fn maximum_cfl(&self) -> f64 {
        self.maximum_cfl
    }

    pub const fn dense_state_bytes(&self) -> u64 {
        self.dense_state_bytes
    }
}

impl<'de> Deserialize<'de> for ClimateSolveReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateSolveReportWire::deserialize(deserializer)?;
        Self::new(
            wire.formation_years,
            wire.macro_steps,
            wire.fast_substeps,
            wire.linear_iterations,
            wire.initial_residual,
            wire.final_residual,
            wire.maximum_cfl,
            wire.dense_state_bytes,
        )
        .map_err(D::Error::custom)
    }
}

/// Global conservation closure after physical sources and sinks are accounted for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateBudgetReport {
    atmosphere_mass_relative_error: f64,
    ocean_volume_relative_error: f64,
    moisture_relative_error: f64,
    energy_relative_error: f64,
    paired_exchange_relative_error: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateBudgetReportWire {
    atmosphere_mass_relative_error: f64,
    ocean_volume_relative_error: f64,
    moisture_relative_error: f64,
    energy_relative_error: f64,
    paired_exchange_relative_error: f64,
}

impl ClimateBudgetReport {
    pub fn new(
        atmosphere_mass_relative_error: f64,
        ocean_volume_relative_error: f64,
        moisture_relative_error: f64,
        energy_relative_error: f64,
        paired_exchange_relative_error: f64,
    ) -> Result<Self, ClimateReportError> {
        let report = Self {
            atmosphere_mass_relative_error,
            ocean_volume_relative_error,
            moisture_relative_error,
            energy_relative_error,
            paired_exchange_relative_error,
        };
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), ClimateReportError> {
        for (field, value, maximum) in [
            (
                "atmosphere_mass_relative_error",
                self.atmosphere_mass_relative_error,
                GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX,
            ),
            (
                "ocean_volume_relative_error",
                self.ocean_volume_relative_error,
                GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX,
            ),
            (
                "moisture_relative_error",
                self.moisture_relative_error,
                GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX,
            ),
            (
                "energy_relative_error",
                self.energy_relative_error,
                GLOBAL_CIRCULATION_ENERGY_RELATIVE_ERROR_MAX,
            ),
            (
                "paired_exchange_relative_error",
                self.paired_exchange_relative_error,
                GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX,
            ),
        ] {
            validate_nonnegative_bounded(field, value, maximum)?;
        }
        Ok(())
    }

    pub const fn atmosphere_mass_relative_error(&self) -> f64 {
        self.atmosphere_mass_relative_error
    }

    pub const fn ocean_volume_relative_error(&self) -> f64 {
        self.ocean_volume_relative_error
    }

    pub const fn moisture_relative_error(&self) -> f64 {
        self.moisture_relative_error
    }

    pub const fn energy_relative_error(&self) -> f64 {
        self.energy_relative_error
    }

    pub const fn paired_exchange_relative_error(&self) -> f64 {
        self.paired_exchange_relative_error
    }
}

impl<'de> Deserialize<'de> for ClimateBudgetReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateBudgetReportWire::deserialize(deserializer)?;
        Self::new(
            wire.atmosphere_mass_relative_error,
            wire.ocean_volume_relative_error,
            wire.moisture_relative_error,
            wire.energy_relative_error,
            wire.paired_exchange_relative_error,
        )
        .map_err(D::Error::custom)
    }
}

/// Conservative surface-bridge closure carried with the public result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateRemapReport {
    forward_source_margin_relative_error: f64,
    forward_target_margin_relative_error: f64,
    reverse_source_margin_relative_error: f64,
    reverse_target_margin_relative_error: f64,
    forward_overlap_count: u32,
    reverse_overlap_count: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateRemapReportWire {
    forward_source_margin_relative_error: f64,
    forward_target_margin_relative_error: f64,
    reverse_source_margin_relative_error: f64,
    reverse_target_margin_relative_error: f64,
    forward_overlap_count: u32,
    reverse_overlap_count: u32,
}

impl ClimateRemapReport {
    pub fn new(
        forward_source_margin_relative_error: f64,
        forward_target_margin_relative_error: f64,
        reverse_source_margin_relative_error: f64,
        reverse_target_margin_relative_error: f64,
        forward_overlap_count: u32,
        reverse_overlap_count: u32,
    ) -> Result<Self, ClimateReportError> {
        let report = Self {
            forward_source_margin_relative_error,
            forward_target_margin_relative_error,
            reverse_source_margin_relative_error,
            reverse_target_margin_relative_error,
            forward_overlap_count,
            reverse_overlap_count,
        };
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), ClimateReportError> {
        for (field, value) in [
            (
                "forward_source_margin_relative_error",
                self.forward_source_margin_relative_error,
            ),
            (
                "forward_target_margin_relative_error",
                self.forward_target_margin_relative_error,
            ),
            (
                "reverse_source_margin_relative_error",
                self.reverse_source_margin_relative_error,
            ),
            (
                "reverse_target_margin_relative_error",
                self.reverse_target_margin_relative_error,
            ),
        ] {
            validate_nonnegative_bounded(field, value, 1.0e-10)?;
        }
        if self.forward_overlap_count == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "forward_overlap_count",
            });
        }
        if self.reverse_overlap_count == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "reverse_overlap_count",
            });
        }
        Ok(())
    }

    pub const fn forward_overlap_count(&self) -> u32 {
        self.forward_overlap_count
    }

    pub const fn reverse_overlap_count(&self) -> u32 {
        self.reverse_overlap_count
    }
}

impl<'de> Deserialize<'de> for ClimateRemapReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateRemapReportWire::deserialize(deserializer)?;
        Self::new(
            wire.forward_source_margin_relative_error,
            wire.forward_target_margin_relative_error,
            wire.reverse_source_margin_relative_error,
            wire.reverse_target_margin_relative_error,
            wire.forward_overlap_count,
            wire.reverse_overlap_count,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_nonnegative_bounded(
    field: &'static str,
    found: f64,
    maximum: f64,
) -> Result<(), ClimateReportError> {
    if !found.is_finite() || found < 0.0 {
        return Err(ClimateReportError::InvalidStatistic { field, found });
    }
    if found > maximum {
        return Err(ClimateReportError::StatisticAboveMaximum {
            field,
            found,
            maximum,
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateReportError {
    #[error("climate report {field} is invalid: {found}")]
    InvalidStatistic { field: &'static str, found: f64 },
    #[error("climate report {field} is zero")]
    ZeroWork { field: &'static str },
    #[error("climate residual increased from {initial} to {final_value}")]
    ResidualIncreased { initial: f64, final_value: f64 },
    #[error("climate report {field} is {found}, maximum {maximum}")]
    StatisticAboveMaximum {
        field: &'static str,
        found: f64,
        maximum: f64,
    },
}

/// Stable semantic monthly fields projected onto the authoritative surface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalCirculationFields {
    near_surface_wind_m_s: MonthlyVector3Field,
    upper_wind_m_s: Option<MonthlyVector3Field>,
    vertical_wind_shear_m_s: Option<MonthlyVector3Field>,
    surface_ocean_current_m_s: MonthlyVector3Field,
    monthly_air_temperature_c: MonthlyScalarField,
    monthly_sea_surface_temperature_c: MonthlyScalarField,
    monthly_thermocline_temperature_c: Option<MonthlyScalarField>,
    monthly_thermocline_depth_m: Option<MonthlyScalarField>,
    monthly_specific_humidity: MonthlyScalarField,
    monthly_precipitation_mm_day: MonthlyScalarField,
    monthly_lower_atmosphere_height_anomaly_m: MonthlyScalarField,
    monthly_upper_atmosphere_height_anomaly_m: Option<MonthlyScalarField>,
    monthly_sea_surface_height_anomaly_m: MonthlyScalarField,
    monthly_thermocline_height_anomaly_m: Option<MonthlyScalarField>,
    monthly_deep_ocean_temperature_c: Option<MonthlyScalarField>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlobalCirculationFieldsWire {
    near_surface_wind_m_s: MonthlyVector3Field,
    upper_wind_m_s: Option<MonthlyVector3Field>,
    vertical_wind_shear_m_s: Option<MonthlyVector3Field>,
    surface_ocean_current_m_s: MonthlyVector3Field,
    monthly_air_temperature_c: MonthlyScalarField,
    monthly_sea_surface_temperature_c: MonthlyScalarField,
    monthly_thermocline_temperature_c: Option<MonthlyScalarField>,
    monthly_thermocline_depth_m: Option<MonthlyScalarField>,
    monthly_specific_humidity: MonthlyScalarField,
    monthly_precipitation_mm_day: MonthlyScalarField,
    monthly_lower_atmosphere_height_anomaly_m: MonthlyScalarField,
    monthly_upper_atmosphere_height_anomaly_m: Option<MonthlyScalarField>,
    monthly_sea_surface_height_anomaly_m: MonthlyScalarField,
    monthly_thermocline_height_anomaly_m: Option<MonthlyScalarField>,
    monthly_deep_ocean_temperature_c: Option<MonthlyScalarField>,
}

impl GlobalCirculationFields {
    #[allow(clippy::too_many_arguments)]
    pub fn new_c1(
        near_surface_wind_m_s: MonthlyVector3Field,
        surface_ocean_current_m_s: MonthlyVector3Field,
        monthly_air_temperature_c: MonthlyScalarField,
        monthly_sea_surface_temperature_c: MonthlyScalarField,
        monthly_specific_humidity: MonthlyScalarField,
        monthly_precipitation_mm_day: MonthlyScalarField,
        monthly_lower_atmosphere_height_anomaly_m: MonthlyScalarField,
        monthly_sea_surface_height_anomaly_m: MonthlyScalarField,
    ) -> Result<Self, GlobalCirculationValidationError> {
        let fields = Self {
            near_surface_wind_m_s,
            upper_wind_m_s: None,
            vertical_wind_shear_m_s: None,
            surface_ocean_current_m_s,
            monthly_air_temperature_c,
            monthly_sea_surface_temperature_c,
            monthly_thermocline_temperature_c: None,
            monthly_thermocline_depth_m: None,
            monthly_specific_humidity,
            monthly_precipitation_mm_day,
            monthly_lower_atmosphere_height_anomaly_m,
            monthly_upper_atmosphere_height_anomaly_m: None,
            monthly_sea_surface_height_anomaly_m,
            monthly_thermocline_height_anomaly_m: None,
            monthly_deep_ocean_temperature_c: None,
        };
        fields.validate(ClimateModelProfile::C1SingleLayerV1, fields.cell_count())?;
        Ok(fields)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_c2(
        near_surface_wind_m_s: MonthlyVector3Field,
        upper_wind_m_s: MonthlyVector3Field,
        vertical_wind_shear_m_s: MonthlyVector3Field,
        surface_ocean_current_m_s: MonthlyVector3Field,
        monthly_air_temperature_c: MonthlyScalarField,
        monthly_sea_surface_temperature_c: MonthlyScalarField,
        monthly_thermocline_temperature_c: MonthlyScalarField,
        monthly_thermocline_depth_m: MonthlyScalarField,
        monthly_specific_humidity: MonthlyScalarField,
        monthly_precipitation_mm_day: MonthlyScalarField,
        monthly_lower_atmosphere_height_anomaly_m: MonthlyScalarField,
        monthly_upper_atmosphere_height_anomaly_m: MonthlyScalarField,
        monthly_sea_surface_height_anomaly_m: MonthlyScalarField,
        monthly_thermocline_height_anomaly_m: MonthlyScalarField,
        monthly_deep_ocean_temperature_c: MonthlyScalarField,
    ) -> Result<Self, GlobalCirculationValidationError> {
        let fields = Self {
            near_surface_wind_m_s,
            upper_wind_m_s: Some(upper_wind_m_s),
            vertical_wind_shear_m_s: Some(vertical_wind_shear_m_s),
            surface_ocean_current_m_s,
            monthly_air_temperature_c,
            monthly_sea_surface_temperature_c,
            monthly_thermocline_temperature_c: Some(monthly_thermocline_temperature_c),
            monthly_thermocline_depth_m: Some(monthly_thermocline_depth_m),
            monthly_specific_humidity,
            monthly_precipitation_mm_day,
            monthly_lower_atmosphere_height_anomaly_m,
            monthly_upper_atmosphere_height_anomaly_m: Some(
                monthly_upper_atmosphere_height_anomaly_m,
            ),
            monthly_sea_surface_height_anomaly_m,
            monthly_thermocline_height_anomaly_m: Some(monthly_thermocline_height_anomaly_m),
            monthly_deep_ocean_temperature_c: Some(monthly_deep_ocean_temperature_c),
        };
        fields.validate(ClimateModelProfile::C2LayeredV1, fields.cell_count())?;
        Ok(fields)
    }

    fn inferred_profile(&self) -> Result<ClimateModelProfile, GlobalCirculationValidationError> {
        let optional_presence = [
            self.upper_wind_m_s.is_some(),
            self.vertical_wind_shear_m_s.is_some(),
            self.monthly_thermocline_temperature_c.is_some(),
            self.monthly_thermocline_depth_m.is_some(),
            self.monthly_upper_atmosphere_height_anomaly_m.is_some(),
            self.monthly_thermocline_height_anomaly_m.is_some(),
            self.monthly_deep_ocean_temperature_c.is_some(),
        ];
        if optional_presence.iter().all(|present| !present) {
            Ok(ClimateModelProfile::C1SingleLayerV1)
        } else if optional_presence.iter().all(|present| *present) {
            Ok(ClimateModelProfile::C2LayeredV1)
        } else {
            Err(GlobalCirculationValidationError::IncompleteVerticalFields)
        }
    }

    pub fn validate(
        &self,
        profile: ClimateModelProfile,
        expected_cells: usize,
    ) -> Result<(), GlobalCirculationValidationError> {
        let inferred = self.inferred_profile()?;
        if inferred != profile {
            return Err(GlobalCirculationValidationError::FieldProfileMismatch {
                fields: inferred,
                snapshot: profile,
            });
        }
        if expected_cells == 0 {
            return Err(GlobalCirculationValidationError::EmptyFields);
        }
        for (field, found) in self.field_lengths() {
            if found != expected_cells {
                return Err(GlobalCirculationValidationError::FieldLengthMismatch {
                    field,
                    found,
                    expected: expected_cells,
                });
            }
        }
        validate_monthly_vector3("near_surface_wind_m_s", &self.near_surface_wind_m_s, 200.0)?;
        validate_monthly_vector3(
            "surface_ocean_current_m_s",
            &self.surface_ocean_current_m_s,
            20.0,
        )?;
        validate_monthly_scalar(
            "monthly_air_temperature_c",
            &self.monthly_air_temperature_c,
            -120.0,
            80.0,
        )?;
        validate_monthly_scalar(
            "monthly_sea_surface_temperature_c",
            &self.monthly_sea_surface_temperature_c,
            -5.0,
            60.0,
        )?;
        validate_monthly_scalar(
            "monthly_specific_humidity",
            &self.monthly_specific_humidity,
            0.0,
            0.1,
        )?;
        validate_monthly_scalar(
            "monthly_precipitation_mm_day",
            &self.monthly_precipitation_mm_day,
            0.0,
            1_000.0,
        )?;
        validate_monthly_scalar(
            "monthly_lower_atmosphere_height_anomaly_m",
            &self.monthly_lower_atmosphere_height_anomaly_m,
            -20_000.0,
            20_000.0,
        )?;
        validate_monthly_scalar(
            "monthly_sea_surface_height_anomaly_m",
            &self.monthly_sea_surface_height_anomaly_m,
            -100.0,
            100.0,
        )?;

        if profile == ClimateModelProfile::C2LayeredV1 {
            let upper = self.upper_wind_m_s.as_ref().expect("inferred C2");
            let shear = self.vertical_wind_shear_m_s.as_ref().expect("inferred C2");
            validate_monthly_vector3("upper_wind_m_s", upper, 200.0)?;
            validate_monthly_vector3("vertical_wind_shear_m_s", shear, 300.0)?;
            validate_shear_identity(&self.near_surface_wind_m_s, upper, shear)?;
            validate_monthly_scalar(
                "monthly_thermocline_temperature_c",
                self.monthly_thermocline_temperature_c
                    .as_ref()
                    .expect("inferred C2"),
                -5.0,
                50.0,
            )?;
            validate_monthly_scalar(
                "monthly_thermocline_depth_m",
                self.monthly_thermocline_depth_m
                    .as_ref()
                    .expect("inferred C2"),
                1.0,
                5_000.0,
            )?;
            validate_monthly_scalar(
                "monthly_upper_atmosphere_height_anomaly_m",
                self.monthly_upper_atmosphere_height_anomaly_m
                    .as_ref()
                    .expect("inferred C2"),
                -20_000.0,
                20_000.0,
            )?;
            validate_monthly_scalar(
                "monthly_thermocline_height_anomaly_m",
                self.monthly_thermocline_height_anomaly_m
                    .as_ref()
                    .expect("inferred C2"),
                -1_000.0,
                1_000.0,
            )?;
            validate_monthly_scalar(
                "monthly_deep_ocean_temperature_c",
                self.monthly_deep_ocean_temperature_c
                    .as_ref()
                    .expect("inferred C2"),
                -5.0,
                40.0,
            )?;
        }
        Ok(())
    }

    fn field_lengths(&self) -> Vec<(&'static str, usize)> {
        let mut lengths = vec![
            ("near_surface_wind_m_s", self.near_surface_wind_m_s.len()),
            (
                "surface_ocean_current_m_s",
                self.surface_ocean_current_m_s.len(),
            ),
            (
                "monthly_air_temperature_c",
                self.monthly_air_temperature_c.len(),
            ),
            (
                "monthly_sea_surface_temperature_c",
                self.monthly_sea_surface_temperature_c.len(),
            ),
            (
                "monthly_specific_humidity",
                self.monthly_specific_humidity.len(),
            ),
            (
                "monthly_precipitation_mm_day",
                self.monthly_precipitation_mm_day.len(),
            ),
            (
                "monthly_lower_atmosphere_height_anomaly_m",
                self.monthly_lower_atmosphere_height_anomaly_m.len(),
            ),
            (
                "monthly_sea_surface_height_anomaly_m",
                self.monthly_sea_surface_height_anomaly_m.len(),
            ),
        ];
        for (name, field) in [
            ("upper_wind_m_s", self.upper_wind_m_s.as_ref()),
            (
                "vertical_wind_shear_m_s",
                self.vertical_wind_shear_m_s.as_ref(),
            ),
        ] {
            if let Some(field) = field {
                lengths.push((name, field.len()));
            }
        }
        for (name, field) in [
            (
                "monthly_thermocline_temperature_c",
                self.monthly_thermocline_temperature_c.as_ref(),
            ),
            (
                "monthly_thermocline_depth_m",
                self.monthly_thermocline_depth_m.as_ref(),
            ),
            (
                "monthly_upper_atmosphere_height_anomaly_m",
                self.monthly_upper_atmosphere_height_anomaly_m.as_ref(),
            ),
            (
                "monthly_thermocline_height_anomaly_m",
                self.monthly_thermocline_height_anomaly_m.as_ref(),
            ),
            (
                "monthly_deep_ocean_temperature_c",
                self.monthly_deep_ocean_temperature_c.as_ref(),
            ),
        ] {
            if let Some(field) = field {
                lengths.push((name, field.len()));
            }
        }
        lengths
    }

    pub fn cell_count(&self) -> usize {
        self.near_surface_wind_m_s.len()
    }

    pub const fn near_surface_wind_m_s(&self) -> &MonthlyVector3Field {
        &self.near_surface_wind_m_s
    }

    pub const fn upper_wind_m_s(&self) -> Option<&MonthlyVector3Field> {
        self.upper_wind_m_s.as_ref()
    }

    pub const fn vertical_wind_shear_m_s(&self) -> Option<&MonthlyVector3Field> {
        self.vertical_wind_shear_m_s.as_ref()
    }

    pub const fn surface_ocean_current_m_s(&self) -> &MonthlyVector3Field {
        &self.surface_ocean_current_m_s
    }

    pub const fn monthly_air_temperature_c(&self) -> &MonthlyScalarField {
        &self.monthly_air_temperature_c
    }

    pub const fn monthly_sea_surface_temperature_c(&self) -> &MonthlyScalarField {
        &self.monthly_sea_surface_temperature_c
    }

    pub const fn monthly_thermocline_temperature_c(&self) -> Option<&MonthlyScalarField> {
        self.monthly_thermocline_temperature_c.as_ref()
    }

    pub const fn monthly_thermocline_depth_m(&self) -> Option<&MonthlyScalarField> {
        self.monthly_thermocline_depth_m.as_ref()
    }

    pub const fn monthly_specific_humidity(&self) -> &MonthlyScalarField {
        &self.monthly_specific_humidity
    }

    pub const fn monthly_precipitation_mm_day(&self) -> &MonthlyScalarField {
        &self.monthly_precipitation_mm_day
    }

    pub const fn monthly_lower_atmosphere_height_anomaly_m(&self) -> &MonthlyScalarField {
        &self.monthly_lower_atmosphere_height_anomaly_m
    }

    pub const fn monthly_upper_atmosphere_height_anomaly_m(&self) -> Option<&MonthlyScalarField> {
        self.monthly_upper_atmosphere_height_anomaly_m.as_ref()
    }

    pub const fn monthly_sea_surface_height_anomaly_m(&self) -> &MonthlyScalarField {
        &self.monthly_sea_surface_height_anomaly_m
    }

    pub const fn monthly_thermocline_height_anomaly_m(&self) -> Option<&MonthlyScalarField> {
        self.monthly_thermocline_height_anomaly_m.as_ref()
    }

    pub const fn monthly_deep_ocean_temperature_c(&self) -> Option<&MonthlyScalarField> {
        self.monthly_deep_ocean_temperature_c.as_ref()
    }
}

impl<'de> Deserialize<'de> for GlobalCirculationFields {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = GlobalCirculationFieldsWire::deserialize(deserializer)?;
        let fields = Self {
            near_surface_wind_m_s: wire.near_surface_wind_m_s,
            upper_wind_m_s: wire.upper_wind_m_s,
            vertical_wind_shear_m_s: wire.vertical_wind_shear_m_s,
            surface_ocean_current_m_s: wire.surface_ocean_current_m_s,
            monthly_air_temperature_c: wire.monthly_air_temperature_c,
            monthly_sea_surface_temperature_c: wire.monthly_sea_surface_temperature_c,
            monthly_thermocline_temperature_c: wire.monthly_thermocline_temperature_c,
            monthly_thermocline_depth_m: wire.monthly_thermocline_depth_m,
            monthly_specific_humidity: wire.monthly_specific_humidity,
            monthly_precipitation_mm_day: wire.monthly_precipitation_mm_day,
            monthly_lower_atmosphere_height_anomaly_m: wire
                .monthly_lower_atmosphere_height_anomaly_m,
            monthly_upper_atmosphere_height_anomaly_m: wire
                .monthly_upper_atmosphere_height_anomaly_m,
            monthly_sea_surface_height_anomaly_m: wire.monthly_sea_surface_height_anomaly_m,
            monthly_thermocline_height_anomaly_m: wire.monthly_thermocline_height_anomaly_m,
            monthly_deep_ocean_temperature_c: wire.monthly_deep_ocean_temperature_c,
        };
        let profile = fields.inferred_profile().map_err(D::Error::custom)?;
        fields
            .validate(profile, fields.cell_count())
            .map_err(D::Error::custom)?;
        Ok(fields)
    }
}

fn validate_monthly_scalar(
    field: &'static str,
    values: &MonthlyScalarField,
    minimum: f32,
    maximum: f32,
) -> Result<(), GlobalCirculationValidationError> {
    for (cell, months) in values.values().iter().enumerate() {
        for (month, value) in months.iter().copied().enumerate() {
            if !value.is_finite() || value < minimum || value > maximum {
                return Err(GlobalCirculationValidationError::ScalarOutOfRange {
                    field,
                    cell,
                    month,
                    found: value,
                    minimum,
                    maximum,
                });
            }
        }
    }
    Ok(())
}

fn validate_monthly_vector3(
    field: &'static str,
    values: &MonthlyVector3Field,
    component_abs_max: f32,
) -> Result<(), GlobalCirculationValidationError> {
    for (cell, months) in values.values().iter().enumerate() {
        for (month, vector) in months.iter().enumerate() {
            for (component, value) in vector.iter().copied().enumerate() {
                if !value.is_finite() || value.abs() > component_abs_max {
                    return Err(GlobalCirculationValidationError::VectorOutOfRange {
                        field,
                        cell,
                        month,
                        component,
                        found: value,
                        component_abs_max,
                    });
                }
            }
        }
    }
    Ok(())
}

fn validate_shear_identity(
    lower: &MonthlyVector3Field,
    upper: &MonthlyVector3Field,
    shear: &MonthlyVector3Field,
) -> Result<(), GlobalCirculationValidationError> {
    for cell in 0..lower.len() {
        for month in 0..CLIMATE_MONTH_COUNT {
            for component in 0..3 {
                let expected =
                    upper.values()[cell][month][component] - lower.values()[cell][month][component];
                let found = shear.values()[cell][month][component];
                if (found - expected).abs() > 2.0e-4 {
                    return Err(GlobalCirculationValidationError::ShearIdentityMismatch {
                        cell,
                        month,
                        component,
                        found,
                        expected,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Immutable P4 seasonal atmosphere-ocean facts on the authoritative sphere.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalCirculationSnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    layout: ClimateLayerLayout,
    integrator: ProductionIntegratorId,
    capabilities: ClimateCapabilitySet,
    checkpoint: ClimateCheckpoint,
    solve_report: ClimateSolveReport,
    budget_report: ClimateBudgetReport,
    remap_report: ClimateRemapReport,
    fields: GlobalCirculationFields,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlobalCirculationSnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    layout: ClimateLayerLayout,
    integrator: ProductionIntegratorId,
    capabilities: ClimateCapabilitySet,
    checkpoint: ClimateCheckpoint,
    solve_report: ClimateSolveReport,
    budget_report: ClimateBudgetReport,
    remap_report: ClimateRemapReport,
    fields: GlobalCirculationFields,
}

impl GlobalCirculationSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        layout: ClimateLayerLayout,
        integrator: ProductionIntegratorId,
        capabilities: ClimateCapabilitySet,
        checkpoint: ClimateCheckpoint,
        solve_report: ClimateSolveReport,
        budget_report: ClimateBudgetReport,
        remap_report: ClimateRemapReport,
        fields: GlobalCirculationFields,
    ) -> Result<Self, GlobalCirculationValidationError> {
        let snapshot = Self {
            schema_version,
            surface_ref,
            layout,
            integrator,
            capabilities,
            checkpoint,
            solve_report,
            budget_report,
            remap_report,
            fields,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks invariants that require only serialized identities and fields.
    pub fn validate(&self) -> Result<(), GlobalCirculationValidationError> {
        if self.schema_version != GLOBAL_CIRCULATION_SCHEMA_V1 {
            return Err(GlobalCirculationValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: GLOBAL_CIRCULATION_SCHEMA_V1,
            });
        }
        self.surface_ref.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "surface_ref",
                reason: error.to_string(),
            }
        })?;
        if self.surface_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(GlobalCirculationValidationError::InvalidSurfaceKind {
                found: self.surface_ref.geometry_kind(),
            });
        }
        self.layout.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "layout",
                reason: error.to_string(),
            }
        })?;
        self.capabilities.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "capabilities",
                reason: error.to_string(),
            }
        })?;
        self.checkpoint.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "checkpoint",
                reason: error.to_string(),
            }
        })?;
        self.solve_report.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "solve_report",
                reason: error.to_string(),
            }
        })?;
        self.budget_report.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "budget_report",
                reason: error.to_string(),
            }
        })?;
        self.remap_report.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "remap_report",
                reason: error.to_string(),
            }
        })?;

        let profile = self.layout.profile();
        if self.checkpoint.profile() != profile {
            return Err(
                GlobalCirculationValidationError::CheckpointIdentityMismatch { field: "profile" },
            );
        }
        if self.checkpoint.integrator() != self.integrator {
            return Err(
                GlobalCirculationValidationError::CheckpointIdentityMismatch {
                    field: "integrator",
                },
            );
        }
        if self.checkpoint.model_fingerprint() != &self.layout.fingerprint() {
            return Err(
                GlobalCirculationValidationError::CheckpointIdentityMismatch {
                    field: "model_fingerprint",
                },
            );
        }
        if self.capabilities != ClimateCapabilitySet::for_profile(profile) {
            return Err(GlobalCirculationValidationError::CapabilityProfileMismatch { profile });
        }
        self.fields
            .validate(profile, self.surface_ref.cell_count() as usize)?;
        Ok(())
    }

    /// Rechecks exact surface identity and every published vector's tangency.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), GlobalCirculationValidationError> {
        self.validate()?;
        surface
            .validate()
            .map_err(|error| GlobalCirculationValidationError::InvalidNested {
                role: "authoritative_surface",
                reason: error.to_string(),
            })?;
        let authoritative = SurfaceRef::for_spherical(surface);
        if authoritative != self.surface_ref {
            return Err(GlobalCirculationValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        validate_tangent_field(
            "near_surface_wind_m_s",
            self.fields.near_surface_wind_m_s(),
            surface,
        )?;
        validate_tangent_field(
            "surface_ocean_current_m_s",
            self.fields.surface_ocean_current_m_s(),
            surface,
        )?;
        if let Some(field) = self.fields.upper_wind_m_s() {
            validate_tangent_field("upper_wind_m_s", field, surface)?;
        }
        if let Some(field) = self.fields.vertical_wind_shear_m_s() {
            validate_tangent_field("vertical_wind_shear_m_s", field, surface)?;
        }
        Ok(())
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    pub const fn profile(&self) -> ClimateModelProfile {
        self.layout.profile()
    }

    pub const fn layout(&self) -> &ClimateLayerLayout {
        &self.layout
    }

    pub const fn integrator(&self) -> ProductionIntegratorId {
        self.integrator
    }

    pub const fn capabilities(&self) -> &ClimateCapabilitySet {
        &self.capabilities
    }

    pub const fn checkpoint(&self) -> &ClimateCheckpoint {
        &self.checkpoint
    }

    pub const fn solve_report(&self) -> &ClimateSolveReport {
        &self.solve_report
    }

    pub const fn budget_report(&self) -> &ClimateBudgetReport {
        &self.budget_report
    }

    pub const fn remap_report(&self) -> &ClimateRemapReport {
        &self.remap_report
    }

    pub const fn fields(&self) -> &GlobalCirculationFields {
        &self.fields
    }
}

impl<'de> Deserialize<'de> for GlobalCirculationSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = GlobalCirculationSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            wire.layout,
            wire.integrator,
            wire.capabilities,
            wire.checkpoint,
            wire.solve_report,
            wire.budget_report,
            wire.remap_report,
            wire.fields,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_tangent_field(
    field: &'static str,
    values: &MonthlyVector3Field,
    surface: &SphericalSurfaceSnapshot,
) -> Result<(), GlobalCirculationValidationError> {
    for (cell, record) in surface.cells().iter().enumerate() {
        let radial = record.centroid.components();
        for (month, vector) in values.values()[cell].iter().enumerate() {
            let radial_component = f64::from(vector[0]) * radial[0]
                + f64::from(vector[1]) * radial[1]
                + f64::from(vector[2]) * radial[2];
            if radial_component.abs() > GLOBAL_CIRCULATION_TANGENCY_TOLERANCE_M_S {
                return Err(GlobalCirculationValidationError::NonTangentVector {
                    field,
                    cell: record.id,
                    month,
                    radial_component,
                });
            }
        }
    }
    Ok(())
}

/// Invalid public layered climate data or contradictory numerical evidence.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum GlobalCirculationValidationError {
    #[error("unsupported global circulation schema {found}; supported schema is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("invalid global circulation {role}: {reason}")]
    InvalidNested { role: &'static str, reason: String },
    #[error(
        "global circulation requires a spherical Voronoi V1 authoritative surface, found {found:?}"
    )]
    InvalidSurfaceKind { found: SurfaceGeometryKind },
    #[error("global circulation fields cannot be empty")]
    EmptyFields,
    #[error("vertical fields must be either the complete C2 set or all absent for C1")]
    IncompleteVerticalFields,
    #[error("field set implies {fields:?}, but snapshot declares {snapshot:?}")]
    FieldProfileMismatch {
        fields: ClimateModelProfile,
        snapshot: ClimateModelProfile,
    },
    #[error("{field} has {found} cells, expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        found: usize,
        expected: usize,
    },
    #[error("{field}[{cell}][{month}]={found} is outside {minimum}..={maximum}")]
    ScalarOutOfRange {
        field: &'static str,
        cell: usize,
        month: usize,
        found: f32,
        minimum: f32,
        maximum: f32,
    },
    #[error("{field}[{cell}][{month}][{component}]={found} exceeds component magnitude {component_abs_max}")]
    VectorOutOfRange {
        field: &'static str,
        cell: usize,
        month: usize,
        component: usize,
        found: f32,
        component_abs_max: f32,
    },
    #[error("vertical shear identity failed at cell {cell}, month {month}, component {component}: {found} != {expected}")]
    ShearIdentityMismatch {
        cell: usize,
        month: usize,
        component: usize,
        found: f32,
        expected: f32,
    },
    #[error("checkpoint {field} does not match snapshot identity")]
    CheckpointIdentityMismatch { field: &'static str },
    #[error("capabilities do not equal the locked P4 inventory for {profile:?}")]
    CapabilityProfileMismatch { profile: ClimateModelProfile },
    #[error(
        "snapshot surface {snapshot:?} does not match authoritative surface {authoritative:?}"
    )]
    SurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
    #[error("{field} at {cell:?}, month {month} has radial component {radial_component} m/s")]
    NonTangentVector {
        field: &'static str,
        cell: CellId,
        month: usize,
        radial_component: f64,
    },
}

/// A cubed-sphere climate grid and the exact conservative bridges to one
/// authoritative geodesic surface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateWorkDomainSnapshot {
    schema_version: u16,
    profile: NaturalQualityProfile,
    face_resolution: u16,
    source_ref: SurfaceRef,
    climate_grid_fingerprint: [u8; 32],
    climate_surface: SphericalSurfaceSnapshot,
    source_to_climate: ConservativeSurfaceMap,
    climate_to_source: ConservativeSurfaceMap,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateWorkDomainSnapshotWire {
    schema_version: u16,
    profile: NaturalQualityProfile,
    face_resolution: u16,
    source_ref: SurfaceRef,
    climate_grid_fingerprint: [u8; 32],
    climate_surface: SphericalSurfaceSnapshot,
    source_to_climate: ConservativeSurfaceMap,
    climate_to_source: ConservativeSurfaceMap,
}

impl ClimateWorkDomainSnapshot {
    /// Constructs the domain only after all cross-object identities close.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        profile: NaturalQualityProfile,
        face_resolution: u16,
        source_ref: SurfaceRef,
        climate_grid_fingerprint: [u8; 32],
        climate_surface: SphericalSurfaceSnapshot,
        source_to_climate: ConservativeSurfaceMap,
        climate_to_source: ConservativeSurfaceMap,
    ) -> Result<Self, ClimateWorkDomainValidationError> {
        let snapshot = Self {
            schema_version,
            profile,
            face_resolution,
            source_ref,
            climate_grid_fingerprint,
            climate_surface,
            source_to_climate,
            climate_to_source,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks the self-contained schema, topology counts, and map identities.
    pub fn validate(&self) -> Result<(), ClimateWorkDomainValidationError> {
        if self.schema_version != CLIMATE_WORK_DOMAIN_SCHEMA_V1 {
            return Err(ClimateWorkDomainValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: CLIMATE_WORK_DOMAIN_SCHEMA_V1,
            });
        }
        let expected_resolution = self.profile.climate_face_resolution();
        if self.face_resolution != expected_resolution {
            return Err(ClimateWorkDomainValidationError::FaceResolutionMismatch {
                profile: self.profile,
                found: self.face_resolution,
                expected: expected_resolution,
            });
        }
        self.source_ref.validate().map_err(|error| {
            ClimateWorkDomainValidationError::InvalidSourceRef {
                reason: error.to_string(),
            }
        })?;
        if self.source_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(ClimateWorkDomainValidationError::NonSphericalSource);
        }
        if self.climate_grid_fingerprint == [0; 32] {
            return Err(ClimateWorkDomainValidationError::ZeroGridFingerprint);
        }
        self.climate_surface.validate().map_err(|error| {
            ClimateWorkDomainValidationError::InvalidClimateSurface {
                reason: error.to_string(),
            }
        })?;

        let resolution = u32::from(self.face_resolution);
        let expected_cells = 6_u32 * resolution * resolution;
        let expected_edges = 2 * expected_cells;
        let expected_vertices = expected_cells + 2;
        let found_cells = self.climate_surface.cells().len() as u32;
        let found_edges = self.climate_surface.edges().len() as u32;
        let found_vertices = self.climate_surface.vertices().len() as u32;
        if (found_cells, found_edges, found_vertices)
            != (expected_cells, expected_edges, expected_vertices)
        {
            return Err(ClimateWorkDomainValidationError::CubedSphereCountMismatch {
                found_cells,
                found_edges,
                found_vertices,
                expected_cells,
                expected_edges,
                expected_vertices,
            });
        }

        self.source_to_climate.validate().map_err(|error| {
            ClimateWorkDomainValidationError::InvalidMap {
                role: "source_to_climate",
                reason: error.to_string(),
            }
        })?;
        self.climate_to_source.validate().map_err(|error| {
            ClimateWorkDomainValidationError::InvalidMap {
                role: "climate_to_source",
                reason: error.to_string(),
            }
        })?;
        let climate_ref = SurfaceRef::for_spherical(&self.climate_surface);
        validate_map_identity(
            "source_to_climate",
            &self.source_to_climate,
            self.source_ref,
            climate_ref,
        )?;
        validate_map_identity(
            "climate_to_source",
            &self.climate_to_source,
            climate_ref,
            self.source_ref,
        )?;
        Ok(())
    }

    /// Binds the serialized source identity and radius to the supplied surface.
    pub fn validate_against(
        &self,
        source: &SphericalSurfaceSnapshot,
    ) -> Result<(), ClimateWorkDomainValidationError> {
        self.validate()?;
        source.validate().map_err(|error| {
            ClimateWorkDomainValidationError::InvalidAuthoritativeSurface {
                reason: error.to_string(),
            }
        })?;
        let found_ref = SurfaceRef::for_spherical(source);
        if found_ref != self.source_ref {
            return Err(ClimateWorkDomainValidationError::SourceMismatch {
                stored: self.source_ref,
                found: found_ref,
            });
        }
        if source.radius().get().to_bits() != self.climate_surface.radius().get().to_bits() {
            return Err(ClimateWorkDomainValidationError::RadiusMismatch {
                source_m: source.radius().get(),
                climate_m: self.climate_surface.radius().get(),
            });
        }
        Ok(())
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn profile(&self) -> NaturalQualityProfile {
        self.profile
    }

    pub const fn face_resolution(&self) -> u16 {
        self.face_resolution
    }

    pub const fn source_ref(&self) -> SurfaceRef {
        self.source_ref
    }

    pub const fn climate_grid_fingerprint(&self) -> &[u8; 32] {
        &self.climate_grid_fingerprint
    }

    pub const fn climate_surface(&self) -> &SphericalSurfaceSnapshot {
        &self.climate_surface
    }

    pub const fn source_to_climate(&self) -> &ConservativeSurfaceMap {
        &self.source_to_climate
    }

    pub const fn climate_to_source(&self) -> &ConservativeSurfaceMap {
        &self.climate_to_source
    }
}

impl<'de> Deserialize<'de> for ClimateWorkDomainSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateWorkDomainSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.profile,
            wire.face_resolution,
            wire.source_ref,
            wire.climate_grid_fingerprint,
            wire.climate_surface,
            wire.source_to_climate,
            wire.climate_to_source,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_map_identity(
    role: &'static str,
    map: &ConservativeSurfaceMap,
    expected_source: SurfaceRef,
    expected_target: SurfaceRef,
) -> Result<(), ClimateWorkDomainValidationError> {
    if map.source_ref() != expected_source || map.target_ref() != expected_target {
        return Err(ClimateWorkDomainValidationError::MapIdentityMismatch { role });
    }
    Ok(())
}

/// Invalid serialized or cross-linked climate work-domain data.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateWorkDomainValidationError {
    #[error("unsupported climate work-domain schema {found}; supported schema is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("{profile:?} climate face resolution is {found}, expected {expected}")]
    FaceResolutionMismatch {
        profile: NaturalQualityProfile,
        found: u16,
        expected: u16,
    },
    #[error("invalid authoritative source identity: {reason}")]
    InvalidSourceRef { reason: String },
    #[error("the climate work-domain source must be spherical")]
    NonSphericalSource,
    #[error("the climate work-grid fingerprint cannot be zero")]
    ZeroGridFingerprint,
    #[error("invalid climate surface: {reason}")]
    InvalidClimateSurface { reason: String },
    #[error("cubed-sphere counts are cells={found_cells}, edges={found_edges}, vertices={found_vertices}; expected cells={expected_cells}, edges={expected_edges}, vertices={expected_vertices}")]
    CubedSphereCountMismatch {
        found_cells: u32,
        found_edges: u32,
        found_vertices: u32,
        expected_cells: u32,
        expected_edges: u32,
        expected_vertices: u32,
    },
    #[error("invalid {role} conservative map: {reason}")]
    InvalidMap { role: &'static str, reason: String },
    #[error("{role} map source/target identity does not match the work domain")]
    MapIdentityMismatch { role: &'static str },
    #[error("invalid supplied authoritative surface: {reason}")]
    InvalidAuthoritativeSurface { reason: String },
    #[error("work-domain source identity {stored:?} does not match supplied surface {found:?}")]
    SourceMismatch {
        stored: SurfaceRef,
        found: SurfaceRef,
    },
    #[error("authoritative radius {source_m} m differs from climate radius {climate_m} m")]
    RadiusMismatch { source_m: f64, climate_m: f64 },
}
