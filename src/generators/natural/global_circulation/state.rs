use thiserror::Error;

use crate::generators::natural::circulation::CubedSphereGrid;
use crate::world::natural::{
    ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, PlanetForcing, CLIMATE_MONTH_COUNT,
};

const C1_ACTIVE_ROLES: [ClimateLayerRole; 2] = [
    ClimateLayerRole::LowerAtmosphere,
    ClimateLayerRole::OceanMixedLayer,
];
const C2_ACTIVE_ROLES: [ClimateLayerRole; 4] = [
    ClimateLayerRole::LowerAtmosphere,
    ClimateLayerRole::UpperAtmosphere,
    ClimateLayerRole::OceanMixedLayer,
    ClimateLayerRole::OceanThermocline,
];

#[derive(Debug, Clone, PartialEq)]
struct ActiveLayerState {
    role: ClimateLayerRole,
    reference_thickness_m: f32,
    height_anomaly_m: Vec<f32>,
    velocity_m_s: Vec<[f32; 3]>,
    temperature_c: Vec<f32>,
}

/// Reusable instantaneous state for one fixed C1 or C2 monthly solve.
#[derive(Debug, Clone, PartialEq)]
pub struct LayeredClimateState {
    profile: ClimateModelProfile,
    grid_fingerprint: [u8; 32],
    cell_count: usize,
    active_layers: Vec<ActiveLayerState>,
    specific_humidity: Vec<f32>,
    deep_ocean_temperature_c: Option<Vec<f32>>,
}

impl LayeredClimateState {
    pub fn from_forcing(
        grid: &CubedSphereGrid,
        layout: &ClimateLayerLayout,
        forcing: &PlanetForcing,
        month: usize,
    ) -> Result<Self, LayeredStateError> {
        layout
            .validate()
            .map_err(|error| LayeredStateError::InvalidInput {
                role: "layout",
                reason: error.to_string(),
            })?;
        forcing
            .validate()
            .map_err(|error| LayeredStateError::InvalidInput {
                role: "forcing",
                reason: error.to_string(),
            })?;
        if month >= CLIMATE_MONTH_COUNT {
            return Err(LayeredStateError::InvalidMonth { found: month });
        }
        if forcing.grid_fingerprint() != grid.fingerprint()
            || forcing.cell_count() != grid.cell_count()
        {
            return Err(LayeredStateError::GridMismatch);
        }

        let mut active_layers = Vec::with_capacity(Self::roles_for_profile(layout.profile()).len());
        for role in Self::roles_for_profile(layout.profile()) {
            let layer = layout
                .layers()
                .iter()
                .find(|layer| layer.role() == *role)
                .expect("fixed layout contains every active role");
            let temperature_c = match role {
                ClimateLayerRole::LowerAtmosphere => forcing
                    .equilibrium_air_temperature_c()
                    .iter()
                    .map(|months| months[month])
                    .collect(),
                ClimateLayerRole::UpperAtmosphere => forcing
                    .equilibrium_air_temperature_c()
                    .iter()
                    .map(|months| months[month] - 12.0)
                    .collect(),
                ClimateLayerRole::OceanMixedLayer => forcing
                    .equilibrium_surface_temperature_c()
                    .iter()
                    .map(|months| months[month])
                    .collect(),
                ClimateLayerRole::OceanThermocline => forcing
                    .equilibrium_surface_temperature_c()
                    .iter()
                    .map(|months| months[month] - 8.0)
                    .collect(),
                ClimateLayerRole::DeepOceanReservoir => {
                    unreachable!("deep reservoir is not dynamically active")
                }
            };
            active_layers.push(ActiveLayerState {
                role: *role,
                reference_thickness_m: layer.reference_thickness_m() as f32,
                height_anomaly_m: vec![0.0; grid.cell_count()],
                velocity_m_s: vec![[0.0; 3]; grid.cell_count()],
                temperature_c,
            });
        }
        let specific_humidity = forcing
            .equilibrium_specific_humidity()
            .iter()
            .map(|months| months[month])
            .collect();
        let deep_ocean_temperature_c =
            (layout.profile() == ClimateModelProfile::C2LayeredV1).then(|| {
                forcing
                    .equilibrium_surface_temperature_c()
                    .iter()
                    .map(|months| (months[month] - 12.0).clamp(-5.0, 40.0))
                    .collect()
            });
        let state = Self {
            profile: layout.profile(),
            grid_fingerprint: *grid.fingerprint(),
            cell_count: grid.cell_count(),
            active_layers,
            specific_humidity,
            deep_ocean_temperature_c,
        };
        state.validate_against(grid)?;
        Ok(state)
    }

    pub fn validate_against(&self, grid: &CubedSphereGrid) -> Result<(), LayeredStateError> {
        if self.grid_fingerprint != *grid.fingerprint() || self.cell_count != grid.cell_count() {
            return Err(LayeredStateError::GridMismatch);
        }
        let roles = Self::roles_for_profile(self.profile);
        if self.active_layers.len() != roles.len()
            || self
                .active_layers
                .iter()
                .zip(roles)
                .any(|(layer, role)| layer.role != *role)
        {
            return Err(LayeredStateError::RoleInventoryMismatch);
        }
        if self.specific_humidity.len() != self.cell_count {
            return Err(LayeredStateError::FieldLengthMismatch {
                field: "specific_humidity",
                found: self.specific_humidity.len(),
                expected: self.cell_count,
            });
        }
        for layer in &self.active_layers {
            for (field, found) in [
                ("height_anomaly_m", layer.height_anomaly_m.len()),
                ("velocity_m_s", layer.velocity_m_s.len()),
                ("temperature_c", layer.temperature_c.len()),
            ] {
                if found != self.cell_count {
                    return Err(LayeredStateError::FieldLengthMismatch {
                        field,
                        found,
                        expected: self.cell_count,
                    });
                }
            }
            for (cell, value) in layer.height_anomaly_m.iter().copied().enumerate() {
                let thickness = layer.reference_thickness_m + value;
                if !value.is_finite() || !thickness.is_finite() || thickness <= 0.0 {
                    return Err(LayeredStateError::NonPositiveLayerThickness {
                        role: layer.role,
                        cell,
                        found: thickness,
                    });
                }
            }
            for (cell, value) in layer.temperature_c.iter().copied().enumerate() {
                if !value.is_finite() {
                    return Err(LayeredStateError::NonFiniteScalar {
                        field: "temperature_c",
                        cell,
                    });
                }
            }
            for (cell, vector) in layer.velocity_m_s.iter().enumerate() {
                if vector.iter().any(|value| !value.is_finite()) {
                    return Err(LayeredStateError::NonFiniteVector {
                        field: "velocity_m_s",
                        cell,
                    });
                }
            }
        }
        for (cell, humidity) in self.specific_humidity.iter().copied().enumerate() {
            if !humidity.is_finite() || humidity < 0.0 {
                return Err(LayeredStateError::InvalidHumidity {
                    cell,
                    found: humidity,
                });
            }
        }
        match (self.profile, &self.deep_ocean_temperature_c) {
            (ClimateModelProfile::C1SingleLayerV1, None)
            | (ClimateModelProfile::C2LayeredV1, Some(_)) => {}
            _ => return Err(LayeredStateError::DeepReservoirProfileMismatch),
        }
        if let Some(values) = &self.deep_ocean_temperature_c {
            if values.len() != self.cell_count {
                return Err(LayeredStateError::FieldLengthMismatch {
                    field: "deep_ocean_temperature_c",
                    found: values.len(),
                    expected: self.cell_count,
                });
            }
            for (cell, value) in values.iter().enumerate() {
                if !value.is_finite() {
                    return Err(LayeredStateError::NonFiniteScalar {
                        field: "deep_ocean_temperature_c",
                        cell,
                    });
                }
            }
        }
        Ok(())
    }

    const fn roles_for_profile(profile: ClimateModelProfile) -> &'static [ClimateLayerRole] {
        match profile {
            ClimateModelProfile::C1SingleLayerV1 => &C1_ACTIVE_ROLES,
            ClimateModelProfile::C2LayeredV1 => &C2_ACTIVE_ROLES,
        }
    }

    fn layer(&self, role: ClimateLayerRole) -> Option<&ActiveLayerState> {
        self.active_layers.iter().find(|layer| layer.role == role)
    }

    fn layer_mut(&mut self, role: ClimateLayerRole) -> Option<&mut ActiveLayerState> {
        self.active_layers
            .iter_mut()
            .find(|layer| layer.role == role)
    }

    pub const fn profile(&self) -> ClimateModelProfile {
        self.profile
    }

    pub const fn grid_fingerprint(&self) -> &[u8; 32] {
        &self.grid_fingerprint
    }

    pub const fn cell_count(&self) -> usize {
        self.cell_count
    }

    pub fn active_roles(&self) -> &'static [ClimateLayerRole] {
        Self::roles_for_profile(self.profile)
    }

    pub fn reference_thickness_m(&self, role: ClimateLayerRole) -> Option<f32> {
        self.layer(role).map(|layer| layer.reference_thickness_m)
    }

    pub fn actual_thickness_m(&self, role: ClimateLayerRole) -> Option<Vec<f32>> {
        self.layer(role).map(|layer| {
            layer
                .height_anomaly_m
                .iter()
                .map(|value| layer.reference_thickness_m + value)
                .collect()
        })
    }

    pub fn height_anomaly_m(&self, role: ClimateLayerRole) -> Option<&[f32]> {
        self.layer(role)
            .map(|layer| layer.height_anomaly_m.as_slice())
    }

    pub fn height_anomaly_m_mut(&mut self, role: ClimateLayerRole) -> Option<&mut [f32]> {
        self.layer_mut(role)
            .map(|layer| layer.height_anomaly_m.as_mut_slice())
    }

    pub fn velocity_m_s(&self, role: ClimateLayerRole) -> Option<&[[f32; 3]]> {
        self.layer(role).map(|layer| layer.velocity_m_s.as_slice())
    }

    pub fn velocity_m_s_mut(&mut self, role: ClimateLayerRole) -> Option<&mut [[f32; 3]]> {
        self.layer_mut(role)
            .map(|layer| layer.velocity_m_s.as_mut_slice())
    }

    pub fn temperature_c(&self, role: ClimateLayerRole) -> Option<&[f32]> {
        self.layer(role).map(|layer| layer.temperature_c.as_slice())
    }

    pub fn temperature_c_mut(&mut self, role: ClimateLayerRole) -> Option<&mut [f32]> {
        self.layer_mut(role)
            .map(|layer| layer.temperature_c.as_mut_slice())
    }

    pub fn specific_humidity(&self) -> &[f32] {
        &self.specific_humidity
    }

    pub fn specific_humidity_mut(&mut self) -> &mut [f32] {
        &mut self.specific_humidity
    }

    pub fn deep_ocean_temperature_c(&self) -> Option<&[f32]> {
        self.deep_ocean_temperature_c.as_deref()
    }

    pub fn deep_ocean_temperature_c_mut(&mut self) -> Option<&mut [f32]> {
        self.deep_ocean_temperature_c.as_deref_mut()
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum LayeredStateError {
    #[error("invalid layered-state {role}: {reason}")]
    InvalidInput { role: &'static str, reason: String },
    #[error("month {found} is outside the 12-month climatology")]
    InvalidMonth { found: usize },
    #[error("layered state and forcing do not match the cubed-sphere grid")]
    GridMismatch,
    #[error("layered state role inventory does not match its fixed profile")]
    RoleInventoryMismatch,
    #[error("{field} has {found} cells, expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        found: usize,
        expected: usize,
    },
    #[error("{role:?} cell {cell} has non-positive actual thickness {found} m")]
    NonPositiveLayerThickness {
        role: ClimateLayerRole,
        cell: usize,
        found: f32,
    },
    #[error("{field} cell {cell} is non-finite")]
    NonFiniteScalar { field: &'static str, cell: usize },
    #[error("{field} vector at cell {cell} is non-finite")]
    NonFiniteVector { field: &'static str, cell: usize },
    #[error("specific humidity at cell {cell} is invalid: {found}")]
    InvalidHumidity { cell: usize, found: f32 },
    #[error("deep-ocean reservoir presence does not match the model profile")]
    DeepReservoirProfileMismatch,
}
