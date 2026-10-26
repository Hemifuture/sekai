//! Minimal seed-dependent forcing of the reduced climate (design section 10 T3).
//!
//! The reduced model needs only four seed-dependent quantities from the terrain: the land
//! fraction and scheme-C reference height `H = M h` of every work cell (LOD-2 section 3.2), the
//! land evapotranspiration share, and the published land mask of every authoritative cell.
//! Everything else the C2 forcing assembled (equilibrium targets, ocean depth, wet-edge
//! permeability, sea-ice prior, fingerprints) has no reader in the reduced model. Monthly
//! insolation depends on the user's axial tilt, so it is evaluated here rather than cached
//! with the seed-independent geometry.

use thiserror::Error;

use super::ReducedClimateGeometry;
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::CirculationOperatorError;
use crate::generators::spatial::{remap_intensive_f32_cancellable, ConservativeRemapError};
use crate::world::natural::{
    atmospheric_reference_surface_height_m, formation_runoff_fraction,
    monthly_daily_mean_insolation_fraction, ClimateSpec, ClimateWorkDomainSnapshot, LandOceanKind,
    SurfaceWaterGeometry, CLIMATE_MONTH_COUNT,
};

/// Authoritative-surface terrain the reduced forcing reads; every slice has one value per
/// authoritative cell.
#[derive(Debug, Clone, Copy)]
pub(in crate::generators::natural) struct ReducedClimateTerrain<'a> {
    /// Current surface elevation, m.
    pub elevation_m: &'a [f32],
    /// Sea level and per-cell ocean area fraction of the same terrain.
    pub surface_water_geometry: &'a SurfaceWaterGeometry,
    /// Relative substrate permeability in `[0, 1]`.
    pub relative_permeability: &'a [f32],
}

/// The reduced model's seed-dependent inputs on the work grid, plus the authoritative land
/// mask and reference heights that projection reads.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::generators::natural) struct ReducedClimateForcing {
    /// Land share of each work cell.
    pub(super) land_fraction: Vec<f64>,
    /// Scheme-C reference height `H = M h` of each work cell, m.
    pub(super) reference_height_m: Vec<f64>,
    /// Tangent gradient of the reference height on the work grid, m/m.
    pub(super) reference_height_gradient: Vec<[f64; 3]>,
    /// Land evapotranspiration share of precipitation, `f_land (1 - runoff(kappa))` (the A3
    /// recycling convention that design section 18.3 replaces with a Budyko curve).
    pub(super) land_evapotranspiration_fraction: Vec<f64>,
    /// Daily-mean insolation fraction of every month on each work cell.
    pub(super) monthly_insolation_fraction: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    /// Atmospheric reference height `h` of each authoritative cell, m (LOD-2 section 3.1).
    pub(super) cell_reference_height_m: Vec<f64>,
    /// Whether each authoritative cell is published as land.
    pub(super) cell_publishes_land: Vec<bool>,
}

impl ReducedClimateForcing {
    /// Builds the forcing of one terrain on a work domain and its reduced geometry.
    ///
    /// # Errors
    ///
    /// Returns [`ReducedClimateForcingError::Cancelled`] on cancellation,
    /// [`ReducedClimateForcingError::InvalidInput`] for an invalid climate spec or a terrain
    /// slice whose length is not the domain's source cell count, and the remap or gradient
    /// error otherwise.
    pub(in crate::generators::natural) fn build(
        domain: &ClimateWorkDomainSnapshot,
        geometry: &ReducedClimateGeometry,
        terrain: ReducedClimateTerrain<'_>,
        climate_spec: &ClimateSpec,
        cancellation: &BuildCancellation,
    ) -> Result<Self, ReducedClimateForcingError> {
        climate_spec
            .validate()
            .map_err(|error| ReducedClimateForcingError::InvalidInput {
                role: "climate_spec",
                reason: error.to_string(),
            })?;
        let ocean_fraction = terrain.surface_water_geometry.ocean_area_fraction();
        let cells = ocean_fraction.len();
        for (role, found) in [
            ("elevation_m", terrain.elevation_m.len()),
            ("relative_permeability", terrain.relative_permeability.len()),
        ] {
            if found != cells {
                return Err(ReducedClimateForcingError::InvalidInput {
                    role,
                    reason: format!("expected {cells} cells, found {found}"),
                });
            }
        }
        check_cancelled(cancellation)?;

        let sea_level_m = terrain.surface_water_geometry.sea_level_m();
        let mut source_land_fraction = Vec::with_capacity(cells);
        let mut source_reference_height_m = Vec::with_capacity(cells);
        let mut source_evapotranspiration = Vec::with_capacity(cells);
        let mut cell_publishes_land = Vec::with_capacity(cells);
        for ((&elevation, &ocean), &permeability) in terrain
            .elevation_m
            .iter()
            .zip(ocean_fraction)
            .zip(terrain.relative_permeability)
        {
            let land = 1.0 - ocean;
            source_land_fraction.push(land);
            source_reference_height_m
                .push(atmospheric_reference_surface_height_m(elevation - sea_level_m, land) as f32);
            source_evapotranspiration
                .push((f64::from(land) * (1.0 - formation_runoff_fraction(permeability))) as f32);
            cell_publishes_land.push(
                LandOceanKind::classify_exact(f64::from(elevation), f64::from(sea_level_m))
                    == LandOceanKind::Land,
            );
        }
        check_cancelled(cancellation)?;

        let map = domain.source_to_climate();
        let cancelled = || cancellation.is_cancelled();
        let remap = |values: &[f32]| {
            remap_intensive_f32_cancellable(map, values, &cancelled).map_err(map_remap_error)
        };
        let land_fraction = remap(&source_land_fraction)?;
        let reference_height = remap(&source_reference_height_m)?;
        let land_evapotranspiration_fraction = remap(&source_evapotranspiration)?;
        let reference_height_gradient = geometry
            .operators()
            .gradient_cancellable(&reference_height, cancellation)
            .map_err(map_operator_error)?
            .into_iter()
            .map(|gradient| gradient.map(f64::from))
            .collect();

        let axial_tilt_rad = f64::from(climate_spec.axial_tilt_degrees()).to_radians();
        let monthly_insolation_fraction = geometry
            .latitude_rad
            .iter()
            .map(|&latitude| monthly_daily_mean_insolation_fraction(latitude, axial_tilt_rad))
            .collect();
        let widen = |values: Vec<f32>| values.into_iter().map(f64::from).collect::<Vec<_>>();
        Ok(Self {
            land_fraction: widen(land_fraction),
            reference_height_m: widen(reference_height),
            reference_height_gradient,
            land_evapotranspiration_fraction: widen(land_evapotranspiration_fraction),
            monthly_insolation_fraction,
            cell_reference_height_m: widen(source_reference_height_m),
            cell_publishes_land,
        })
    }
}

/// Failures of the reduced forcing build.
#[derive(Debug, Clone, PartialEq, Error)]
pub(in crate::generators::natural) enum ReducedClimateForcingError {
    #[error("reduced climate forcing build was cancelled")]
    Cancelled,
    #[error("invalid reduced climate forcing input {role}: {reason}")]
    InvalidInput { role: &'static str, reason: String },
    #[error("reduced climate forcing remap failed: {reason}")]
    Remap { reason: String },
    #[error("reduced climate forcing gradient failed: {reason}")]
    Gradient { reason: String },
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), ReducedClimateForcingError> {
    if cancellation.is_cancelled() {
        Err(ReducedClimateForcingError::Cancelled)
    } else {
        Ok(())
    }
}

fn map_remap_error(error: ConservativeRemapError) -> ReducedClimateForcingError {
    if error == ConservativeRemapError::Cancelled {
        ReducedClimateForcingError::Cancelled
    } else {
        ReducedClimateForcingError::Remap {
            reason: error.to_string(),
        }
    }
}

fn map_operator_error(error: CirculationOperatorError) -> ReducedClimateForcingError {
    if error == CirculationOperatorError::Cancelled {
        ReducedClimateForcingError::Cancelled
    } else {
        ReducedClimateForcingError::Gradient {
            reason: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::natural::surface_water_geometry::build_surface_water_geometry;
    use crate::generators::natural::ClimateWorkDomainArtifact;
    use crate::generators::spatial::ProfileSurfaceBuilder;
    use crate::world::natural::NaturalQualityProfile;
    use crate::world::Meters;

    /// Contract (design section 10 T3): on a synthetic continent the forcing's work-grid
    /// shares are proper fractions, the evapotranspiration share never exceeds the land share,
    /// the reference height is non-negative and zero where the work cell is all ocean, and the
    /// published land mask is the exact land/ocean classification of each cell.
    #[test]
    fn synthetic_continent_forcing_is_physically_bounded() {
        let cancellation = BuildCancellation::new();
        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &cancellation,
        )
        .unwrap();
        let surface = bundle.authoritative_surface();
        let domain =
            ClimateWorkDomainArtifact::build(surface, NaturalQualityProfile::Draft, &cancellation)
                .unwrap();
        // A raised northern-hemisphere cap with relief rising toward the pole.
        let elevation: Vec<f32> = surface
            .cells()
            .iter()
            .map(|cell| (6_000.0 * cell.centroid.components()[2] - 2_000.0) as f32)
            .collect();
        let water = build_surface_water_geometry(surface, &elevation, 0.0, &cancellation).unwrap();
        let permeability = vec![0.5_f32; elevation.len()];
        let forcing = ReducedClimateForcing::build(
            domain.snapshot(),
            domain.reduced_climate_geometry(),
            ReducedClimateTerrain {
                elevation_m: &elevation,
                surface_water_geometry: &water,
                relative_permeability: &permeability,
            },
            &ClimateSpec::default(),
            &cancellation,
        )
        .unwrap();
        for cell in 0..forcing.land_fraction.len() {
            let land = forcing.land_fraction[cell];
            assert!((0.0..=1.0).contains(&land));
            assert!(forcing.land_evapotranspiration_fraction[cell] <= land + 1.0e-6);
            assert!(forcing.reference_height_m[cell] >= 0.0);
            if land == 0.0 {
                assert_eq!(forcing.reference_height_m[cell], 0.0);
            }
            assert!(forcing.monthly_insolation_fraction[cell]
                .iter()
                .all(|fraction| (0.0..=1.0).contains(fraction)));
        }
        for (cell, &land) in forcing.cell_publishes_land.iter().enumerate() {
            assert_eq!(land, elevation[cell] > 0.0, "cell {cell}");
        }
        assert!(forcing.land_fraction.contains(&1.0));
        assert!(forcing.land_fraction.contains(&0.0));
    }
}
