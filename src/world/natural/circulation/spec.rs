use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// The only supported circulation contract and fingerprint schema.
pub const CIRCULATION_SCHEMA_V1: u16 = 1;
/// The largest supported resolution of one cubed-sphere face.
pub const MAX_CUBED_SPHERE_FACE_RESOLUTION: u16 = 64;

const MAX_STEADY_ITERATIONS: u16 = 4_096;
const MAX_FORMATION_YEARS: u16 = 64;

/// Validated planetary constants and numerical budgets shared by every solver.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CirculationSpec {
    pub face_resolution: u16,
    pub planet_radius_m: f64,
    pub rotation_rate_rad_s: f64,
    pub gravity_m_s2: f64,
    pub atmosphere_reference_depth_m: f32,
    pub atmosphere_reduced_gravity_m_s2: f32,
    pub ocean_reference_depth_m: f32,
    pub ocean_reduced_gravity_m_s2: f32,
    pub atmosphere_drag_s_inv: f32,
    pub ocean_drag_s_inv: f32,
    pub layer_relaxation_s_inv: f32,
    pub thermal_relaxation_s_inv: f32,
    pub max_steady_iterations: u16,
    pub max_formation_years: u16,
    pub convergence_tolerance: f32,
    pub cfl_limit: f32,
}

impl Default for CirculationSpec {
    fn default() -> Self {
        Self {
            face_resolution: 24,
            planet_radius_m: 6_371_000.0,
            rotation_rate_rad_s: 7.292_115_9e-5,
            gravity_m_s2: 9.806_65,
            atmosphere_reference_depth_m: 8_000.0,
            atmosphere_reduced_gravity_m_s2: 0.3125,
            ocean_reference_depth_m: 500.0,
            ocean_reduced_gravity_m_s2: 0.02,
            atmosphere_drag_s_inv: 2.314_814_8e-6,
            ocean_drag_s_inv: 3.858_024_7e-7,
            layer_relaxation_s_inv: 7.716_049_5e-7,
            thermal_relaxation_s_inv: 3.858_024_7e-7,
            max_steady_iterations: 128,
            max_formation_years: 5,
            convergence_tolerance: 1.0e-4,
            cfl_limit: 0.45,
        }
    }
}

impl CirculationSpec {
    /// Validates both physical domains and bounded-work numerical budgets.
    pub fn validate(&self) -> Result<(), CirculationSpecError> {
        if !(1..=MAX_CUBED_SPHERE_FACE_RESOLUTION).contains(&self.face_resolution) {
            return Err(CirculationSpecError::FaceResolutionOutOfRange {
                found: self.face_resolution,
                min: 1,
                max: MAX_CUBED_SPHERE_FACE_RESOLUTION,
            });
        }

        validate_f64(
            "planet_radius_m",
            self.planet_radius_m,
            100_000.0,
            100_000_000.0,
        )?;
        validate_f64("rotation_rate_rad_s", self.rotation_rate_rad_s, 0.0, 0.01)?;
        validate_f64("gravity_m_s2", self.gravity_m_s2, 0.1, 100.0)?;
        validate_f32(
            "atmosphere_reference_depth_m",
            self.atmosphere_reference_depth_m,
            100.0,
            100_000.0,
        )?;
        validate_f32(
            "atmosphere_reduced_gravity_m_s2",
            self.atmosphere_reduced_gravity_m_s2,
            1.0e-5,
            self.gravity_m_s2 as f32,
        )?;
        validate_f32(
            "ocean_reference_depth_m",
            self.ocean_reference_depth_m,
            1.0,
            20_000.0,
        )?;
        validate_f32(
            "ocean_reduced_gravity_m_s2",
            self.ocean_reduced_gravity_m_s2,
            1.0e-5,
            self.gravity_m_s2 as f32,
        )?;
        for (field, value) in [
            ("atmosphere_drag_s_inv", self.atmosphere_drag_s_inv),
            ("ocean_drag_s_inv", self.ocean_drag_s_inv),
            ("layer_relaxation_s_inv", self.layer_relaxation_s_inv),
            ("thermal_relaxation_s_inv", self.thermal_relaxation_s_inv),
        ] {
            validate_f32(field, value, 1.0e-10, 1.0e-2)?;
        }
        if !(1..=MAX_STEADY_ITERATIONS).contains(&self.max_steady_iterations) {
            return Err(CirculationSpecError::BudgetOutOfRange {
                field: "max_steady_iterations",
                found: self.max_steady_iterations,
                min: 1,
                max: MAX_STEADY_ITERATIONS,
            });
        }
        if !(1..=MAX_FORMATION_YEARS).contains(&self.max_formation_years) {
            return Err(CirculationSpecError::BudgetOutOfRange {
                field: "max_formation_years",
                found: self.max_formation_years,
                min: 1,
                max: MAX_FORMATION_YEARS,
            });
        }
        validate_f32(
            "convergence_tolerance",
            self.convergence_tolerance,
            1.0e-8,
            1.0e-2,
        )?;
        validate_f32("cfl_limit", self.cfl_limit, 0.01, 0.45)?;
        Ok(())
    }

    /// Returns the canonical content identity of this validated specification.
    pub fn fingerprint(&self) -> Result<[u8; 32], CirculationSpecError> {
        self.validate()?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.circulation.spec\0");
        hasher.update(&CIRCULATION_SCHEMA_V1.to_le_bytes());
        hasher.update(&self.face_resolution.to_le_bytes());
        hasher.update(&self.planet_radius_m.to_bits().to_le_bytes());
        hasher.update(&self.rotation_rate_rad_s.to_bits().to_le_bytes());
        hasher.update(&self.gravity_m_s2.to_bits().to_le_bytes());
        for value in [
            self.atmosphere_reference_depth_m,
            self.atmosphere_reduced_gravity_m_s2,
            self.ocean_reference_depth_m,
            self.ocean_reduced_gravity_m_s2,
            self.atmosphere_drag_s_inv,
            self.ocean_drag_s_inv,
            self.layer_relaxation_s_inv,
            self.thermal_relaxation_s_inv,
        ] {
            hasher.update(&value.to_bits().to_le_bytes());
        }
        hasher.update(&self.max_steady_iterations.to_le_bytes());
        hasher.update(&self.max_formation_years.to_le_bytes());
        hasher.update(&self.convergence_tolerance.to_bits().to_le_bytes());
        hasher.update(&self.cfl_limit.to_bits().to_le_bytes());
        Ok(*hasher.finalize().as_bytes())
    }
}

#[derive(Deserialize)]
struct CirculationSpecWire {
    face_resolution: u16,
    planet_radius_m: f64,
    rotation_rate_rad_s: f64,
    gravity_m_s2: f64,
    atmosphere_reference_depth_m: f32,
    atmosphere_reduced_gravity_m_s2: f32,
    ocean_reference_depth_m: f32,
    ocean_reduced_gravity_m_s2: f32,
    atmosphere_drag_s_inv: f32,
    ocean_drag_s_inv: f32,
    layer_relaxation_s_inv: f32,
    thermal_relaxation_s_inv: f32,
    max_steady_iterations: u16,
    max_formation_years: u16,
    convergence_tolerance: f32,
    cfl_limit: f32,
}

impl<'de> Deserialize<'de> for CirculationSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CirculationSpecWire::deserialize(deserializer)?;
        let spec = Self {
            face_resolution: wire.face_resolution,
            planet_radius_m: wire.planet_radius_m,
            rotation_rate_rad_s: wire.rotation_rate_rad_s,
            gravity_m_s2: wire.gravity_m_s2,
            atmosphere_reference_depth_m: wire.atmosphere_reference_depth_m,
            atmosphere_reduced_gravity_m_s2: wire.atmosphere_reduced_gravity_m_s2,
            ocean_reference_depth_m: wire.ocean_reference_depth_m,
            ocean_reduced_gravity_m_s2: wire.ocean_reduced_gravity_m_s2,
            atmosphere_drag_s_inv: wire.atmosphere_drag_s_inv,
            ocean_drag_s_inv: wire.ocean_drag_s_inv,
            layer_relaxation_s_inv: wire.layer_relaxation_s_inv,
            thermal_relaxation_s_inv: wire.thermal_relaxation_s_inv,
            max_steady_iterations: wire.max_steady_iterations,
            max_formation_years: wire.max_formation_years,
            convergence_tolerance: wire.convergence_tolerance,
            cfl_limit: wire.cfl_limit,
        };
        spec.validate().map_err(serde::de::Error::custom)?;
        Ok(spec)
    }
}

fn validate_f64(
    field: &'static str,
    found: f64,
    min: f64,
    max: f64,
) -> Result<(), CirculationSpecError> {
    if !found.is_finite() {
        return Err(CirculationSpecError::NonFinite { field });
    }
    if !(min..=max).contains(&found) {
        return Err(CirculationSpecError::PhysicalValueOutOfRange {
            field,
            found,
            min,
            max,
        });
    }
    Ok(())
}

fn validate_f32(
    field: &'static str,
    found: f32,
    min: f32,
    max: f32,
) -> Result<(), CirculationSpecError> {
    validate_f64(field, f64::from(found), f64::from(min), f64::from(max))
}

/// Errors returned by physical-domain or bounded-work validation.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CirculationSpecError {
    #[error("cubed-sphere face resolution {found} is outside {min}..={max}")]
    FaceResolutionOutOfRange { found: u16, min: u16, max: u16 },
    #[error("circulation parameter {field} must be finite")]
    NonFinite { field: &'static str },
    #[error("circulation parameter {field}={found} is outside {min}..={max}")]
    PhysicalValueOutOfRange {
        field: &'static str,
        found: f64,
        min: f64,
        max: f64,
    },
    #[error("circulation budget {field}={found} is outside {min}..={max}")]
    BudgetOutOfRange {
        field: &'static str,
        found: u16,
        min: u16,
        max: u16,
    },
}
