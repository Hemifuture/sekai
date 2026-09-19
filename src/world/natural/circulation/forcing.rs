use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::world::natural::{CLIMATE_MONTH_COUNT, GLOBAL_CIRCULATION_RADIATIVE_FLUX_MAX_W_M2};

use super::{bounded_vec::BoundedVec, MAX_CIRCULATION_CELL_COUNT};

type ForcingCancellation<'a> = Option<&'a dyn Fn() -> bool>;

/// Immutable terrain, surface, and monthly thermodynamic forcing for one grid.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanetForcing {
    grid_fingerprint: [u8; 32],
    fingerprint: [u8; 32],
    elevation_m: Vec<f32>,
    land_fraction: Vec<f32>,
    ocean_depth_m: Vec<f32>,
    surface_albedo: Vec<f32>,
    surface_moisture_availability: Vec<f32>,
    monthly_absorbed_shortwave_w_m2: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    equilibrium_air_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    equilibrium_surface_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    equilibrium_specific_humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanetForcingWire {
    grid_fingerprint: [u8; 32],
    fingerprint: [u8; 32],
    elevation_m: BoundedVec<f32, MAX_CIRCULATION_CELL_COUNT, 1>,
    land_fraction: BoundedVec<f32, MAX_CIRCULATION_CELL_COUNT, 1>,
    ocean_depth_m: BoundedVec<f32, MAX_CIRCULATION_CELL_COUNT, 1>,
    surface_albedo: BoundedVec<f32, MAX_CIRCULATION_CELL_COUNT, 1>,
    surface_moisture_availability: BoundedVec<f32, MAX_CIRCULATION_CELL_COUNT, 1>,
    monthly_absorbed_shortwave_w_m2:
        BoundedVec<[f32; CLIMATE_MONTH_COUNT], MAX_CIRCULATION_CELL_COUNT, 1>,
    equilibrium_air_temperature_c:
        BoundedVec<[f32; CLIMATE_MONTH_COUNT], MAX_CIRCULATION_CELL_COUNT, 1>,
    equilibrium_surface_temperature_c:
        BoundedVec<[f32; CLIMATE_MONTH_COUNT], MAX_CIRCULATION_CELL_COUNT, 1>,
    equilibrium_specific_humidity:
        BoundedVec<[f32; CLIMATE_MONTH_COUNT], MAX_CIRCULATION_CELL_COUNT, 1>,
}

impl PlanetForcing {
    /// Constructs forcing only when every dense field is aligned and physical.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        grid_fingerprint: [u8; 32],
        elevation_m: Vec<f32>,
        land_fraction: Vec<f32>,
        surface_albedo: Vec<f32>,
        surface_moisture_availability: Vec<f32>,
        monthly_absorbed_shortwave_w_m2: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_air_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_surface_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_specific_humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    ) -> Result<Self, ForcingError> {
        let ocean_depth_m = default_ocean_depth_m(&land_fraction);
        Self::new_impl(
            grid_fingerprint,
            elevation_m,
            land_fraction,
            ocean_depth_m,
            surface_albedo,
            surface_moisture_availability,
            monthly_absorbed_shortwave_w_m2,
            equilibrium_air_temperature_c,
            equilibrium_surface_temperature_c,
            equilibrium_specific_humidity,
            None,
        )
    }

    /// Constructs forcing with an explicit physical bathymetry field.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_ocean_depth(
        grid_fingerprint: [u8; 32],
        elevation_m: Vec<f32>,
        land_fraction: Vec<f32>,
        ocean_depth_m: Vec<f32>,
        surface_albedo: Vec<f32>,
        surface_moisture_availability: Vec<f32>,
        monthly_absorbed_shortwave_w_m2: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_air_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_surface_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_specific_humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    ) -> Result<Self, ForcingError> {
        Self::new_impl(
            grid_fingerprint,
            elevation_m,
            land_fraction,
            ocean_depth_m,
            surface_albedo,
            surface_moisture_availability,
            monthly_absorbed_shortwave_w_m2,
            equilibrium_air_temperature_c,
            equilibrium_surface_temperature_c,
            equilibrium_specific_humidity,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg(test)]
    pub(crate) fn new_cancellable(
        grid_fingerprint: [u8; 32],
        elevation_m: Vec<f32>,
        land_fraction: Vec<f32>,
        surface_albedo: Vec<f32>,
        surface_moisture_availability: Vec<f32>,
        monthly_absorbed_shortwave_w_m2: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_air_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_surface_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_specific_humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, ForcingError> {
        let ocean_depth_m = default_ocean_depth_m(&land_fraction);
        Self::new_impl(
            grid_fingerprint,
            elevation_m,
            land_fraction,
            ocean_depth_m,
            surface_albedo,
            surface_moisture_availability,
            monthly_absorbed_shortwave_w_m2,
            equilibrium_air_temperature_c,
            equilibrium_surface_temperature_c,
            equilibrium_specific_humidity,
            Some(cancelled),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_cancellable_with_ocean_depth(
        grid_fingerprint: [u8; 32],
        elevation_m: Vec<f32>,
        land_fraction: Vec<f32>,
        ocean_depth_m: Vec<f32>,
        surface_albedo: Vec<f32>,
        surface_moisture_availability: Vec<f32>,
        monthly_absorbed_shortwave_w_m2: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_air_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_surface_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_specific_humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, ForcingError> {
        Self::new_impl(
            grid_fingerprint,
            elevation_m,
            land_fraction,
            ocean_depth_m,
            surface_albedo,
            surface_moisture_availability,
            monthly_absorbed_shortwave_w_m2,
            equilibrium_air_temperature_c,
            equilibrium_surface_temperature_c,
            equilibrium_specific_humidity,
            Some(cancelled),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_impl(
        grid_fingerprint: [u8; 32],
        elevation_m: Vec<f32>,
        land_fraction: Vec<f32>,
        ocean_depth_m: Vec<f32>,
        surface_albedo: Vec<f32>,
        surface_moisture_availability: Vec<f32>,
        monthly_absorbed_shortwave_w_m2: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_air_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_surface_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        equilibrium_specific_humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        cancellation: ForcingCancellation<'_>,
    ) -> Result<Self, ForcingError> {
        let mut forcing = Self {
            grid_fingerprint,
            fingerprint: [0; 32],
            elevation_m,
            land_fraction,
            ocean_depth_m,
            surface_albedo,
            surface_moisture_availability,
            monthly_absorbed_shortwave_w_m2,
            equilibrium_air_temperature_c,
            equilibrium_surface_temperature_c,
            equilibrium_specific_humidity,
        };
        forcing.validate_content(cancellation)?;
        forcing.fingerprint = forcing.calculate_fingerprint(cancellation)?;
        Ok(forcing)
    }

    /// Revalidates the content and its stored content identity.
    pub fn validate(&self) -> Result<(), ForcingError> {
        self.validate_impl(None)
    }

    pub(crate) fn validate_cancellable(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), ForcingError> {
        self.validate_impl(Some(cancelled))
    }

    fn validate_impl(&self, cancellation: ForcingCancellation<'_>) -> Result<(), ForcingError> {
        self.validate_content(cancellation)?;
        if self.fingerprint != self.calculate_fingerprint(cancellation)? {
            return Err(ForcingError::FingerprintMismatch);
        }
        check_cancelled(cancellation)?;
        Ok(())
    }

    pub fn cell_count(&self) -> usize {
        self.elevation_m.len()
    }

    pub const fn grid_fingerprint(&self) -> &[u8; 32] {
        &self.grid_fingerprint
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub fn elevation_m(&self) -> &[f32] {
        &self.elevation_m
    }

    pub fn land_fraction(&self) -> &[f32] {
        &self.land_fraction
    }

    pub fn ocean_depth_m(&self) -> &[f32] {
        &self.ocean_depth_m
    }

    pub fn surface_albedo(&self) -> &[f32] {
        &self.surface_albedo
    }

    pub fn surface_moisture_availability(&self) -> &[f32] {
        &self.surface_moisture_availability
    }

    pub fn monthly_absorbed_shortwave_w_m2(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.monthly_absorbed_shortwave_w_m2
    }

    pub fn equilibrium_air_temperature_c(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.equilibrium_air_temperature_c
    }

    pub fn equilibrium_surface_temperature_c(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.equilibrium_surface_temperature_c
    }

    pub fn equilibrium_specific_humidity(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.equilibrium_specific_humidity
    }

    fn validate_content(&self, cancellation: ForcingCancellation<'_>) -> Result<(), ForcingError> {
        check_cancelled(cancellation)?;
        let expected = self.elevation_m.len();
        if expected == 0 || expected > MAX_CIRCULATION_CELL_COUNT {
            return Err(ForcingError::CellCountOutOfRange {
                found: expected,
                min: 1,
                max: MAX_CIRCULATION_CELL_COUNT,
            });
        }
        for (field, found) in [
            ("ocean_depth_m", self.ocean_depth_m.len()),
            ("land_fraction", self.land_fraction.len()),
            ("surface_albedo", self.surface_albedo.len()),
            (
                "surface_moisture_availability",
                self.surface_moisture_availability.len(),
            ),
            (
                "monthly_absorbed_shortwave_w_m2",
                self.monthly_absorbed_shortwave_w_m2.len(),
            ),
            (
                "equilibrium_air_temperature_c",
                self.equilibrium_air_temperature_c.len(),
            ),
            (
                "equilibrium_surface_temperature_c",
                self.equilibrium_surface_temperature_c.len(),
            ),
            (
                "equilibrium_specific_humidity",
                self.equilibrium_specific_humidity.len(),
            ),
        ] {
            if found != expected {
                return Err(ForcingError::FieldLengthMismatch {
                    field,
                    expected,
                    found,
                });
            }
        }

        validate_scalar_field("elevation_m", &self.elevation_m, None, cancellation)?;
        validate_scalar_field(
            "ocean_depth_m",
            &self.ocean_depth_m,
            Some((0.0, 20_000.0)),
            cancellation,
        )?;
        validate_scalar_field(
            "land_fraction",
            &self.land_fraction,
            Some((0.0, 1.0)),
            cancellation,
        )?;
        validate_scalar_field(
            "surface_albedo",
            &self.surface_albedo,
            Some((0.0, 1.0)),
            cancellation,
        )?;
        validate_scalar_field(
            "surface_moisture_availability",
            &self.surface_moisture_availability,
            Some((0.0, 1.0)),
            cancellation,
        )?;
        validate_monthly_field(
            "monthly_absorbed_shortwave_w_m2",
            &self.monthly_absorbed_shortwave_w_m2,
            Some((0.0, GLOBAL_CIRCULATION_RADIATIVE_FLUX_MAX_W_M2 as f32)),
            cancellation,
        )?;
        validate_monthly_field(
            "equilibrium_air_temperature_c",
            &self.equilibrium_air_temperature_c,
            None,
            cancellation,
        )?;
        validate_monthly_field(
            "equilibrium_surface_temperature_c",
            &self.equilibrium_surface_temperature_c,
            None,
            cancellation,
        )?;
        validate_monthly_field(
            "equilibrium_specific_humidity",
            &self.equilibrium_specific_humidity,
            Some((0.0, 1.0)),
            cancellation,
        )?;
        check_cancelled(cancellation)?;
        Ok(())
    }

    fn calculate_fingerprint(
        &self,
        cancellation: ForcingCancellation<'_>,
    ) -> Result<[u8; 32], ForcingError> {
        check_cancelled(cancellation)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.planet-forcing.v2\0");
        hasher.update(&super::CIRCULATION_SCHEMA_V1.to_le_bytes());
        hasher.update(&self.grid_fingerprint);
        hasher.update(&(self.cell_count() as u32).to_le_bytes());
        hash_scalars(&mut hasher, &self.elevation_m, cancellation)?;
        hash_scalars(&mut hasher, &self.land_fraction, cancellation)?;
        hash_scalars(&mut hasher, &self.ocean_depth_m, cancellation)?;
        hash_scalars(&mut hasher, &self.surface_albedo, cancellation)?;
        hash_scalars(
            &mut hasher,
            &self.surface_moisture_availability,
            cancellation,
        )?;
        hash_monthly(
            &mut hasher,
            &self.monthly_absorbed_shortwave_w_m2,
            cancellation,
        )?;
        hash_monthly(
            &mut hasher,
            &self.equilibrium_air_temperature_c,
            cancellation,
        )?;
        hash_monthly(
            &mut hasher,
            &self.equilibrium_surface_temperature_c,
            cancellation,
        )?;
        hash_monthly(
            &mut hasher,
            &self.equilibrium_specific_humidity,
            cancellation,
        )?;
        check_cancelled(cancellation)?;
        Ok(*hasher.finalize().as_bytes())
    }
}

impl<'de> Deserialize<'de> for PlanetForcing {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = PlanetForcingWire::deserialize(deserializer)?;
        let forcing = Self {
            grid_fingerprint: wire.grid_fingerprint,
            fingerprint: wire.fingerprint,
            elevation_m: wire.elevation_m.into_vec(),
            land_fraction: wire.land_fraction.into_vec(),
            ocean_depth_m: wire.ocean_depth_m.into_vec(),
            surface_albedo: wire.surface_albedo.into_vec(),
            surface_moisture_availability: wire.surface_moisture_availability.into_vec(),
            monthly_absorbed_shortwave_w_m2: wire.monthly_absorbed_shortwave_w_m2.into_vec(),
            equilibrium_air_temperature_c: wire.equilibrium_air_temperature_c.into_vec(),
            equilibrium_surface_temperature_c: wire.equilibrium_surface_temperature_c.into_vec(),
            equilibrium_specific_humidity: wire.equilibrium_specific_humidity.into_vec(),
        };
        forcing.validate().map_err(serde::de::Error::custom)?;
        Ok(forcing)
    }
}

fn default_ocean_depth_m(land_fraction: &[f32]) -> Vec<f32> {
    land_fraction
        .iter()
        .map(|land| 4_000.0 * (1.0 - *land).clamp(0.0, 1.0))
        .collect()
}

fn validate_scalar_field(
    field: &'static str,
    values: &[f32],
    range: Option<(f32, f32)>,
    cancellation: ForcingCancellation<'_>,
) -> Result<(), ForcingError> {
    for (cell, value) in values.iter().copied().enumerate() {
        poll_cancelled(cell, cancellation)?;
        validate_value(field, cell, None, value, range)?;
    }
    Ok(())
}

fn validate_monthly_field(
    field: &'static str,
    values: &[[f32; CLIMATE_MONTH_COUNT]],
    range: Option<(f32, f32)>,
    cancellation: ForcingCancellation<'_>,
) -> Result<(), ForcingError> {
    for (cell, months) in values.iter().enumerate() {
        poll_cancelled(cell, cancellation)?;
        for (month, value) in months.iter().copied().enumerate() {
            validate_value(field, cell, Some(month), value, range)?;
        }
    }
    Ok(())
}

fn validate_value(
    field: &'static str,
    cell: usize,
    month: Option<usize>,
    value: f32,
    range: Option<(f32, f32)>,
) -> Result<(), ForcingError> {
    if !value.is_finite() {
        return Err(ForcingError::NonFiniteValue { field, cell, month });
    }
    if let Some((min, max)) = range {
        if !(min..=max).contains(&value) {
            return Err(ForcingError::ValueOutOfRange {
                field,
                cell,
                month,
                found: value,
                min,
                max,
            });
        }
    }
    Ok(())
}

fn hash_scalars(
    hasher: &mut blake3::Hasher,
    values: &[f32],
    cancellation: ForcingCancellation<'_>,
) -> Result<(), ForcingError> {
    for (index, value) in values.iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        hasher.update(&value.to_bits().to_le_bytes());
    }
    Ok(())
}

fn hash_monthly(
    hasher: &mut blake3::Hasher,
    values: &[[f32; CLIMATE_MONTH_COUNT]],
    cancellation: ForcingCancellation<'_>,
) -> Result<(), ForcingError> {
    for (index, months) in values.iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        for value in months {
            hasher.update(&value.to_bits().to_le_bytes());
        }
    }
    Ok(())
}

fn poll_cancelled(index: usize, cancellation: ForcingCancellation<'_>) -> Result<(), ForcingError> {
    if index % 256 == 0 {
        check_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_cancelled(cancellation: ForcingCancellation<'_>) -> Result<(), ForcingError> {
    if cancellation.is_some_and(|cancelled| cancelled()) {
        Err(ForcingError::Cancelled)
    } else {
        Ok(())
    }
}

/// Errors returned when forcing is sparse, nonphysical, or not content-authentic.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ForcingError {
    #[error("forcing validation or fingerprinting was cancelled")]
    Cancelled,
    #[error("forcing cell count {found} is outside {min}..={max}")]
    CellCountOutOfRange {
        found: usize,
        min: usize,
        max: usize,
    },
    #[error("forcing field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("forcing field {field} has a non-finite value at cell {cell}, month {month:?}")]
    NonFiniteValue {
        field: &'static str,
        cell: usize,
        month: Option<usize>,
    },
    #[error(
        "forcing field {field} value {found} at cell {cell}, month {month:?} is outside {min}..={max}"
    )]
    ValueOutOfRange {
        field: &'static str,
        cell: usize,
        month: Option<usize>,
        found: f32,
        min: f32,
        max: f32,
    },
    #[error("stored forcing fingerprint does not match canonical field content")]
    FingerprintMismatch,
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn cancellable_constructor_stops_inside_dense_validation_or_hashing() {
        let cells = 4_096;
        let observations = AtomicUsize::new(0);
        let cancelled = || observations.fetch_add(1, Ordering::Relaxed) >= 24;

        let result = PlanetForcing::new_cancellable(
            [7; 32],
            vec![0.0; cells],
            vec![0.5; cells],
            vec![0.2; cells],
            vec![0.8; cells],
            vec![[240.0; CLIMATE_MONTH_COUNT]; cells],
            vec![[10.0; CLIMATE_MONTH_COUNT]; cells],
            vec![[12.0; CLIMATE_MONTH_COUNT]; cells],
            vec![[0.01; CLIMATE_MONTH_COUNT]; cells],
            &cancelled,
        );

        assert_eq!(result, Err(ForcingError::Cancelled));
        assert!(observations.load(Ordering::Relaxed) > 24);
    }
}
