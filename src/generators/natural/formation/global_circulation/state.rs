use thiserror::Error;

use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::CubedSphereGrid;
use crate::world::natural::{
    atmospheric_reference_surface_height_m, saturation_specific_humidity_kg_kg, ClimateLayerLayout,
    ClimateLayerRole, ClimateModelProfile, ForcingError, PlanetForcing, CLIMATE_MONTH_COUNT,
    CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M,
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
pub(crate) const LIQUID_MIXED_LAYER_MIN_C: f32 = -2.0;
pub(crate) const SUBSURFACE_OCEAN_MIN_C: f32 = -5.0;
pub(crate) const OCEAN_EQUILIBRIUM_MAX_C: f32 = 40.0;
pub(crate) const UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C: f32 = 12.0;
pub(crate) const THERMOCLINE_EQUILIBRIUM_OFFSET_C: f32 = 8.0;
pub(crate) const DEEP_OCEAN_EQUILIBRIUM_OFFSET_C: f32 = 12.0;
pub(crate) const UPPER_SPECIFIC_HUMIDITY_INITIAL_FRACTION: f32 = 0.35;

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
    upper_specific_humidity: Option<Vec<f32>>,
    deep_ocean_temperature_c: Option<Vec<f32>>,
}

impl LayeredClimateState {
    pub(crate) fn clone_cancellable(
        &self,
        cancellation: &BuildCancellation,
    ) -> Result<Self, LayeredStateError> {
        check_state_cancelled(Some(cancellation))?;
        let mut active_layers = Vec::with_capacity(self.active_layers.len());
        for layer in &self.active_layers {
            active_layers.push(ActiveLayerState {
                role: layer.role,
                reference_thickness_m: layer.reference_thickness_m,
                height_anomaly_m: copy_scalars_cancellable(&layer.height_anomaly_m, cancellation)?,
                velocity_m_s: copy_vectors_cancellable(&layer.velocity_m_s, cancellation)?,
                temperature_c: copy_scalars_cancellable(&layer.temperature_c, cancellation)?,
            });
        }
        let state = Self {
            profile: self.profile,
            grid_fingerprint: self.grid_fingerprint,
            cell_count: self.cell_count,
            active_layers,
            specific_humidity: copy_scalars_cancellable(&self.specific_humidity, cancellation)?,
            upper_specific_humidity: self
                .upper_specific_humidity
                .as_deref()
                .map(|values| copy_scalars_cancellable(values, cancellation))
                .transpose()?,
            deep_ocean_temperature_c: self
                .deep_ocean_temperature_c
                .as_deref()
                .map(|values| copy_scalars_cancellable(values, cancellation))
                .transpose()?,
        };
        check_state_cancelled(Some(cancellation))?;
        Ok(state)
    }

    pub(crate) fn enforce_full_land_ocean_velocity(
        &mut self,
        forcing: &PlanetForcing,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredStateError> {
        if forcing.cell_count() != self.cell_count {
            return Err(LayeredStateError::GridMismatch);
        }
        for role in [
            ClimateLayerRole::OceanMixedLayer,
            ClimateLayerRole::OceanThermocline,
        ] {
            if let Some(velocity) = self.velocity_m_s_mut(role) {
                for (cell, (vector, land_fraction)) in
                    velocity.iter_mut().zip(forcing.land_fraction()).enumerate()
                {
                    poll_state_cancelled(cell, Some(cancellation))?;
                    if *land_fraction == 1.0 {
                        *vector = [0.0; 3];
                    }
                }
            }
        }
        Ok(())
    }

    /// Fixture forcings publish sea-level-relative elevation, so the
    /// reference surface height is read against a zero sea level.
    pub fn from_forcing(
        grid: &CubedSphereGrid,
        layout: &ClimateLayerLayout,
        forcing: &PlanetForcing,
        month: usize,
    ) -> Result<Self, LayeredStateError> {
        Self::from_forcing_impl(grid, layout, forcing, 0.0, Some(month), None)
    }

    /// Initializes a periodic climatology from the annual mean boundary
    /// state so no arbitrary calendar phase receives a dynamical head start.
    pub(crate) fn from_annual_mean_forcing_cancellable(
        grid: &CubedSphereGrid,
        layout: &ClimateLayerLayout,
        forcing: &PlanetForcing,
        sea_level_m: f32,
        cancellation: &BuildCancellation,
    ) -> Result<Self, LayeredStateError> {
        Self::from_forcing_impl(grid, layout, forcing, sea_level_m, None, Some(cancellation))
    }

    fn from_forcing_impl(
        grid: &CubedSphereGrid,
        layout: &ClimateLayerLayout,
        forcing: &PlanetForcing,
        sea_level_m: f32,
        month: Option<usize>,
        cancellation: Option<&BuildCancellation>,
    ) -> Result<Self, LayeredStateError> {
        check_state_cancelled(cancellation)?;
        layout
            .validate()
            .map_err(|error| LayeredStateError::InvalidInput {
                role: "layout",
                reason: error.to_string(),
            })?;
        match cancellation {
            Some(cancellation) => forcing.validate_cancellable(&|| cancellation.is_cancelled()),
            None => forcing.validate(),
        }
        .map_err(|error| {
            if error == ForcingError::Cancelled {
                LayeredStateError::Cancelled
            } else {
                LayeredStateError::InvalidInput {
                    role: "forcing",
                    reason: error.to_string(),
                }
            }
        })?;
        if let Some(month) = month {
            if month >= CLIMATE_MONTH_COUNT {
                return Err(LayeredStateError::InvalidMonth { found: month });
            }
        }
        if forcing.grid_fingerprint() != grid.fingerprint()
            || forcing.cell_count() != grid.cell_count()
        {
            return Err(LayeredStateError::GridMismatch);
        }

        let upper_reference_c = upper_atmosphere_reference_air_temperature_c(
            grid,
            forcing,
            sea_level_m,
            month,
            cancellation,
        )?;
        let mut active_layers = Vec::with_capacity(Self::roles_for_profile(layout.profile()).len());
        for role in Self::roles_for_profile(layout.profile()) {
            let layer = layout
                .layers()
                .iter()
                .find(|layer| layer.role() == *role)
                .expect("fixed layout contains every active role");
            let mut temperature_c = Vec::with_capacity(grid.cell_count());
            for (cell, &upper_air_c) in upper_reference_c.iter().enumerate() {
                poll_state_cancelled(cell, cancellation)?;
                let value = forcing_initial_temperature(
                    &forcing.equilibrium_air_temperature_c()[cell],
                    &forcing.equilibrium_surface_temperature_c()[cell],
                    upper_air_c,
                    month,
                    *role,
                );
                temperature_c.push(value);
            }
            active_layers.push(ActiveLayerState {
                role: *role,
                reference_thickness_m: layer.reference_thickness_m() as f32,
                height_anomaly_m: vec![0.0; grid.cell_count()],
                velocity_m_s: vec![[0.0; 3]; grid.cell_count()],
                temperature_c,
            });
        }
        let c2 = layout.profile() == ClimateModelProfile::C2LayeredV1;
        let mut specific_humidity = Vec::with_capacity(grid.cell_count());
        let mut upper_specific_humidity = c2.then(|| Vec::with_capacity(grid.cell_count()));
        let mut deep_ocean_temperature_c = c2.then(|| Vec::with_capacity(grid.cell_count()));
        for (cell, &upper_air_c) in upper_reference_c.iter().enumerate() {
            poll_state_cancelled(cell, cancellation)?;
            let humidity = forcing_initial_humidity(
                &forcing.equilibrium_air_temperature_c()[cell],
                &forcing.equilibrium_specific_humidity()[cell],
                month,
            );
            specific_humidity.push(humidity);
            if let Some(upper) = &mut upper_specific_humidity {
                upper.push(UPPER_SPECIFIC_HUMIDITY_INITIAL_FRACTION * humidity);
            }
            if let Some(deep) = &mut deep_ocean_temperature_c {
                deep.push(forcing_initial_temperature(
                    &forcing.equilibrium_air_temperature_c()[cell],
                    &forcing.equilibrium_surface_temperature_c()[cell],
                    upper_air_c,
                    month,
                    ClimateLayerRole::DeepOceanReservoir,
                ));
            }
        }
        let state = Self {
            profile: layout.profile(),
            grid_fingerprint: *grid.fingerprint(),
            cell_count: grid.cell_count(),
            active_layers,
            specific_humidity,
            upper_specific_humidity,
            deep_ocean_temperature_c,
        };
        match cancellation {
            Some(cancellation) => state.validate_against_cancellable(grid, cancellation)?,
            None => state.validate_against(grid)?,
        }
        Ok(state)
    }

    pub fn validate_against(&self, grid: &CubedSphereGrid) -> Result<(), LayeredStateError> {
        self.validate_against_impl(grid, None)
    }

    pub fn validate_against_cancellable(
        &self,
        grid: &CubedSphereGrid,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredStateError> {
        self.validate_against_impl(grid, Some(cancellation))
    }

    fn validate_against_impl(
        &self,
        grid: &CubedSphereGrid,
        cancellation: Option<&BuildCancellation>,
    ) -> Result<(), LayeredStateError> {
        check_state_cancelled(cancellation)?;
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
                poll_state_cancelled(cell, cancellation)?;
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
                poll_state_cancelled(cell, cancellation)?;
                if !value.is_finite() {
                    return Err(LayeredStateError::NonFiniteScalar {
                        field: "temperature_c",
                        cell,
                    });
                }
            }
            for (cell, vector) in layer.velocity_m_s.iter().enumerate() {
                poll_state_cancelled(cell, cancellation)?;
                if vector.iter().any(|value| !value.is_finite()) {
                    return Err(LayeredStateError::NonFiniteVector {
                        field: "velocity_m_s",
                        cell,
                    });
                }
            }
        }
        for (cell, humidity) in self.specific_humidity.iter().copied().enumerate() {
            poll_state_cancelled(cell, cancellation)?;
            if !humidity.is_finite() || humidity < 0.0 {
                return Err(LayeredStateError::InvalidHumidity {
                    cell,
                    found: humidity,
                });
            }
        }
        match (self.profile, &self.upper_specific_humidity) {
            (ClimateModelProfile::C1SingleLayerV1, None)
            | (ClimateModelProfile::C2LayeredV1, Some(_)) => {}
            _ => return Err(LayeredStateError::UpperMoistureProfileMismatch),
        }
        if let Some(values) = &self.upper_specific_humidity {
            if values.len() != self.cell_count {
                return Err(LayeredStateError::FieldLengthMismatch {
                    field: "upper_specific_humidity",
                    found: values.len(),
                    expected: self.cell_count,
                });
            }
            for (cell, humidity) in values.iter().copied().enumerate() {
                poll_state_cancelled(cell, cancellation)?;
                if !humidity.is_finite() || humidity < 0.0 {
                    return Err(LayeredStateError::InvalidUpperHumidity {
                        cell,
                        found: humidity,
                    });
                }
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
                poll_state_cancelled(cell, cancellation)?;
                if !value.is_finite() {
                    return Err(LayeredStateError::NonFiniteScalar {
                        field: "deep_ocean_temperature_c",
                        cell,
                    });
                }
            }
        }
        check_state_cancelled(cancellation)?;
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

    pub fn upper_specific_humidity(&self) -> Option<&[f32]> {
        self.upper_specific_humidity.as_deref()
    }

    pub fn upper_specific_humidity_mut(&mut self) -> Option<&mut [f32]> {
        self.upper_specific_humidity.as_deref_mut()
    }

    pub fn deep_ocean_temperature_c(&self) -> Option<&[f32]> {
        self.deep_ocean_temperature_c.as_deref()
    }

    pub fn deep_ocean_temperature_c_mut(&mut self) -> Option<&mut [f32]> {
        self.deep_ocean_temperature_c.as_deref_mut()
    }
}

fn forcing_initial_humidity(
    air_temperature_c: &[f32; CLIMATE_MONTH_COUNT],
    specific_humidity: &[f32; CLIMATE_MONTH_COUNT],
    month: Option<usize>,
) -> f32 {
    if let Some(month) = month {
        return specific_humidity[month];
    }
    let relative_humidity = air_temperature_c
        .iter()
        .zip(specific_humidity)
        .map(|(temperature, humidity)| {
            let saturation = saturation_specific_humidity_kg_kg(f64::from(*temperature));
            if saturation > 0.0 {
                f64::from(*humidity) / saturation
            } else {
                0.0
            }
        })
        .sum::<f64>()
        / CLIMATE_MONTH_COUNT as f64;
    let annual_mean_temperature = (air_temperature_c
        .iter()
        .copied()
        .map(f64::from)
        .sum::<f64>()
        / CLIMATE_MONTH_COUNT as f64) as f32;
    (relative_humidity * saturation_specific_humidity_kg_kg(f64::from(annual_mean_temperature)))
        as f32
}

fn forcing_initial_temperature(
    air_months: &[f32; CLIMATE_MONTH_COUNT],
    surface_months: &[f32; CLIMATE_MONTH_COUNT],
    upper_air_c: f32,
    month: Option<usize>,
    role: ClimateLayerRole,
) -> f32 {
    // Annual-mean initialisation averages the targets first and applies the
    // role clamp/offset once (milestone A4 §3.2): averaging clamped months
    // biased polar mixed layers warm by up to 30 K.
    month.map_or_else(
        || {
            let mean = |months: &[f32; CLIMATE_MONTH_COUNT]| {
                (months.iter().copied().map(f64::from).sum::<f64>() / CLIMATE_MONTH_COUNT as f64)
                    as f32
            };
            role_reference_temperature_c(role, mean(air_months), mean(surface_months), upper_air_c)
        },
        |month| {
            role_reference_temperature_c(
                role,
                air_months[month],
                surface_months[month],
                upper_air_c,
            )
        },
    )
}

/// Latitude-band count shared with the work-domain memory inventory.
pub(super) fn axisymmetric_band_count(grid: &CubedSphereGrid) -> usize {
    crate::world::natural::global_circulation_axisymmetric_band_count(grid.face_resolution())
}

/// Band index of every cell, evaluated once per workspace.
pub(super) fn axisymmetric_bands(grid: &CubedSphereGrid) -> Vec<u32> {
    let band_count = axisymmetric_band_count(grid);
    grid.cells()
        .iter()
        .map(|cell| {
            let sine_latitude = cell.center_unit()[2].clamp(-1.0, 1.0);
            let fraction = sine_latitude.asin() / std::f64::consts::PI + 0.5;
            ((fraction * band_count as f64).floor() as usize).min(band_count - 1) as u32
        })
        .collect()
}

/// Upper-atmosphere reference air temperature of every cell.
///
/// A cold non-zonal surface anomaly stays in the lower troposphere: Lindzen &
/// Nigam (1987) and Battisti, Sarachik & Hirst (1999) Eq. (4)-(5) let the eddy
/// temperature decay to zero at the 700 hPa reference level, below this
/// model's layer interface, so the free troposphere above it keeps the
/// zonal-mean structure `T(y, z)`. A warm anomaly does not stay confined: the
/// column convects (Battisti et al. 1999 §2b, convecting regime; Gill 1980
/// first baroclinic mode) and the free troposphere follows the column's own
/// equilibrium stratification, which is the fixed offset this model already
/// declares for every layer pair (Manabe, Smagorinsky & Strickler 1965
/// convective adjustment to a stable reference lapse). The upper layer
/// therefore references the warmer of the latitude-band mean and the local
/// sea-level-reduced air target, re-expressed at the cell's own reference
/// height so it shares the lower layer's terrain-lapse coordinate (the two
/// lapse terms cancel in the interface buoyancy). `month` `None` averages the
/// twelve monthly targets first, as the annual-mean initialisation does for
/// every other role.
pub(super) fn upper_atmosphere_reference_air_temperature_c(
    grid: &CubedSphereGrid,
    forcing: &PlanetForcing,
    sea_level_m: f32,
    month: Option<usize>,
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<f32>, LayeredStateError> {
    let bands = axisymmetric_bands(grid);
    let mut band_area_m2 = vec![0.0_f64; axisymmetric_band_count(grid)];
    let mut band_sum_c_m2 = vec![0.0_f64; band_area_m2.len()];
    let mut sea_level_targets_c = Vec::with_capacity(grid.cell_count());
    let mut lapse_offsets_c = Vec::with_capacity(grid.cell_count());
    for (cell, geometry) in grid.cells().iter().enumerate() {
        poll_state_cancelled(cell, cancellation)?;
        let lapse_offset_c = CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M
            * atmospheric_reference_surface_height_m(
                forcing.elevation_m()[cell] - sea_level_m,
                forcing.land_fraction()[cell],
            );
        let months = &forcing.equilibrium_air_temperature_c()[cell];
        let target_c = month.map_or_else(
            || months.iter().copied().map(f64::from).sum::<f64>() / CLIMATE_MONTH_COUNT as f64,
            |month| f64::from(months[month]),
        );
        let band = bands[cell] as usize;
        band_area_m2[band] += geometry.area_m2();
        band_sum_c_m2[band] += geometry.area_m2() * (target_c + lapse_offset_c);
        sea_level_targets_c.push(target_c + lapse_offset_c);
        lapse_offsets_c.push(lapse_offset_c);
    }
    Ok(sea_level_targets_c
        .iter()
        .zip(&lapse_offsets_c)
        .zip(&bands)
        .map(|((&sea_level_target_c, &lapse_offset_c), &band)| {
            let band_mean_c = band_sum_c_m2[band as usize] / band_area_m2[band as usize];
            (band_mean_c.max(sea_level_target_c) - lapse_offset_c) as f32
        })
        .collect())
}

/// Equilibrium temperature of one role at one cell and month.
///
/// `upper_air_c` is the cell's value from
/// `upper_atmosphere_reference_air_temperature_c`; only the upper atmosphere
/// consults it.
pub(super) fn role_reference_temperature_c(
    role: ClimateLayerRole,
    air_temperature_c: f32,
    surface_temperature_c: f32,
    upper_air_c: f32,
) -> f32 {
    match role {
        ClimateLayerRole::LowerAtmosphere => air_temperature_c,
        ClimateLayerRole::UpperAtmosphere => upper_air_c - UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C,
        ClimateLayerRole::OceanMixedLayer => {
            surface_temperature_c.clamp(LIQUID_MIXED_LAYER_MIN_C, OCEAN_EQUILIBRIUM_MAX_C)
        }
        ClimateLayerRole::OceanThermocline => (surface_temperature_c
            - THERMOCLINE_EQUILIBRIUM_OFFSET_C)
            .clamp(SUBSURFACE_OCEAN_MIN_C, OCEAN_EQUILIBRIUM_MAX_C),
        ClimateLayerRole::DeepOceanReservoir => (surface_temperature_c
            - DEEP_OCEAN_EQUILIBRIUM_OFFSET_C)
            .clamp(SUBSURFACE_OCEAN_MIN_C, OCEAN_EQUILIBRIUM_MAX_C),
    }
}

fn copy_scalars_cancellable(
    values: &[f32],
    cancellation: &BuildCancellation,
) -> Result<Vec<f32>, LayeredStateError> {
    let mut copy = Vec::with_capacity(values.len());
    for (index, value) in values.iter().copied().enumerate() {
        poll_state_cancelled(index, Some(cancellation))?;
        copy.push(value);
    }
    Ok(copy)
}

fn copy_vectors_cancellable(
    values: &[[f32; 3]],
    cancellation: &BuildCancellation,
) -> Result<Vec<[f32; 3]>, LayeredStateError> {
    let mut copy = Vec::with_capacity(values.len());
    for (index, value) in values.iter().copied().enumerate() {
        poll_state_cancelled(index, Some(cancellation))?;
        copy.push(value);
    }
    Ok(copy)
}

fn poll_state_cancelled(
    index: usize,
    cancellation: Option<&BuildCancellation>,
) -> Result<(), LayeredStateError> {
    if index % 256 == 0 {
        check_state_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_state_cancelled(
    cancellation: Option<&BuildCancellation>,
) -> Result<(), LayeredStateError> {
    if cancellation.is_some_and(BuildCancellation::is_cancelled) {
        Err(LayeredStateError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum LayeredStateError {
    #[error("layered state validation was cancelled")]
    Cancelled,
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
    #[error("upper-atmosphere specific humidity at cell {cell} is invalid: {found}")]
    InvalidUpperHumidity { cell: usize, found: f32 },
    #[error("upper-atmosphere moisture presence does not match the model profile")]
    UpperMoistureProfileMismatch,
    #[error("deep-ocean reservoir presence does not match the model profile")]
    DeepReservoirProfileMismatch,
}
