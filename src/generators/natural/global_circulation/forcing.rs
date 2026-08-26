use thiserror::Error;

use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::{
    CirculationOperatorError, CirculationOperators, CubedSphereGrid, CubedSphereGridError,
};
use crate::generators::natural::surface_water_geometry::build_surface_water_geometry;
use crate::generators::spatial::{remap_intensive_f32_cancellable, ConservativeRemapError};
use crate::world::natural::{
    absorbed_shortwave_w_m2, gray_equilibrium_surface_temperature_c,
    saturation_specific_humidity_kg_kg, ClimateSpec, ClimateWorkDomainSnapshot,
    ClimateWorkDomainValidationError, ForcingError, FormationTerrainFields, PlanetForcing,
    PrimaryReliefSnapshot, PrimaryReliefValidationError, SurfaceWaterGeometry, CLIMATE_MONTH_COUNT,
    CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M, P4_HIGHLAND_ALBEDO_RAMP_ONSET_M,
    P4_HIGHLAND_ALBEDO_RAMP_SPAN_M, P4_HIGHLAND_SURFACE_ALBEDO_INCREMENT,
    P4_OPEN_OCEAN_SURFACE_ALBEDO, P4_SNOW_FREE_LAND_SURFACE_ALBEDO_INCREMENT,
    REFERENCE_SURFACE_RELATIVE_HUMIDITY,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};

const MONTH_PHASE_OFFSET: f64 = 0.5;

/// Exact P3-derived boundary and equilibrium forcing on the climate work grid.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalClimateForcing {
    source_ref: SurfaceRef,
    source_relief_fingerprint: [u8; 32],
    climate_spec_fingerprint: [u8; 32],
    fingerprint: [u8; 32],
    sea_level_m: f32,
    source_land_fraction: Vec<f32>,
    planet_forcing: PlanetForcing,
    relative_elevation_m: Vec<f32>,
    ocean_depth_m: Vec<f32>,
    terrain_gradient_m_per_m: Vec<[f32; 3]>,
    ocean_edge_permeability: Vec<f32>,
    monthly_insolation_fraction: Vec<[f32; CLIMATE_MONTH_COUNT]>,
}

impl GlobalClimateForcing {
    pub(crate) fn validate_relief_identity(
        &self,
        relief: &PrimaryReliefSnapshot,
    ) -> Result<(), GlobalClimateForcingError> {
        if relief.surface_ref() != self.source_ref
            || relief_fingerprint_impl(relief, None)? != self.source_relief_fingerprint
        {
            return Err(GlobalClimateForcingError::SourceMismatch);
        }
        Ok(())
    }

    pub(crate) fn validate_relief_identity_cancellable(
        &self,
        relief: &PrimaryReliefSnapshot,
        cancellation: &BuildCancellation,
    ) -> Result<(), GlobalClimateForcingError> {
        check_cancelled(cancellation)?;
        if relief.surface_ref() != self.source_ref
            || relief_fingerprint_cancellable(relief, cancellation)?
                != self.source_relief_fingerprint
        {
            return Err(GlobalClimateForcingError::SourceMismatch);
        }
        Ok(())
    }

    pub fn validate_against(
        &self,
        domain: &ClimateWorkDomainSnapshot,
    ) -> Result<(), GlobalClimateForcingError> {
        domain
            .validate()
            .map_err(GlobalClimateForcingError::WorkDomain)?;
        self.validate_payload_against(domain)
    }

    /// Validates this private-field forcing payload against a domain whose
    /// immutable invariants were already established at construction or
    /// deserialization.
    pub(crate) fn validate_payload_against(
        &self,
        domain: &ClimateWorkDomainSnapshot,
    ) -> Result<(), GlobalClimateForcingError> {
        self.validate_payload_against_impl(domain, None)
    }

    pub(crate) fn validate_payload_against_cancellable(
        &self,
        domain: &ClimateWorkDomainSnapshot,
        cancellation: &BuildCancellation,
    ) -> Result<(), GlobalClimateForcingError> {
        self.validate_payload_against_impl(domain, Some(cancellation))
    }

    fn validate_payload_against_impl(
        &self,
        domain: &ClimateWorkDomainSnapshot,
        cancellation: Option<&BuildCancellation>,
    ) -> Result<(), GlobalClimateForcingError> {
        check_optional_cancelled(cancellation)?;
        match cancellation {
            Some(cancellation) => self
                .planet_forcing
                .validate_cancellable(&|| cancellation.is_cancelled()),
            None => self.planet_forcing.validate(),
        }
        .map_err(map_planet_forcing_error)?;
        if self.source_ref != domain.source_ref() {
            return Err(GlobalClimateForcingError::SourceMismatch);
        }
        if self.planet_forcing.grid_fingerprint() != domain.climate_grid_fingerprint() {
            return Err(GlobalClimateForcingError::GridMismatch);
        }
        let cell_count = domain.climate_surface().cells().len();
        let source_cell_count = domain.source_ref().cell_count() as usize;
        if self.source_land_fraction.len() != source_cell_count {
            return Err(GlobalClimateForcingError::FieldLengthMismatch {
                field: "source_land_fraction",
                found: self.source_land_fraction.len(),
                expected: source_cell_count,
            });
        }
        for (index, value) in self.source_land_fraction.iter().copied().enumerate() {
            poll_optional_cancelled(index, cancellation)?;
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(GlobalClimateForcingError::ValueOutOfRange {
                    field: "source_land_fraction",
                    index,
                    found: value,
                    minimum: 0.0,
                    maximum: 1.0,
                });
            }
        }
        for (field, found) in [
            ("relative_elevation_m", self.relative_elevation_m.len()),
            ("ocean_depth_m", self.ocean_depth_m.len()),
            (
                "terrain_gradient_m_per_m",
                self.terrain_gradient_m_per_m.len(),
            ),
            (
                "monthly_insolation_fraction",
                self.monthly_insolation_fraction.len(),
            ),
        ] {
            if found != cell_count {
                return Err(GlobalClimateForcingError::FieldLengthMismatch {
                    field,
                    found,
                    expected: cell_count,
                });
            }
        }
        if self.ocean_edge_permeability.len() != domain.climate_surface().edges().len() {
            return Err(GlobalClimateForcingError::FieldLengthMismatch {
                field: "ocean_edge_permeability",
                found: self.ocean_edge_permeability.len(),
                expected: domain.climate_surface().edges().len(),
            });
        }
        for (field, values, minimum, maximum) in [
            (
                "ocean_depth_m",
                self.ocean_depth_m.as_slice(),
                0.0,
                20_000.0,
            ),
            (
                "ocean_edge_permeability",
                self.ocean_edge_permeability.as_slice(),
                0.0,
                1.0,
            ),
        ] {
            for (index, value) in values.iter().copied().enumerate() {
                poll_optional_cancelled(index, cancellation)?;
                if !value.is_finite() || value < minimum || value > maximum {
                    return Err(GlobalClimateForcingError::ValueOutOfRange {
                        field,
                        index,
                        found: value,
                        minimum,
                        maximum,
                    });
                }
            }
        }
        if self.planet_forcing.ocean_depth_m() != self.ocean_depth_m {
            return Err(GlobalClimateForcingError::PayloadIdentityMismatch {
                field: "ocean_depth_m",
            });
        }
        for cell in 0..cell_count {
            poll_optional_cancelled(cell, cancellation)?;
            for month in 0..CLIMATE_MONTH_COUNT {
                let expected = absorbed_shortwave_w_m2(
                    f64::from(self.monthly_insolation_fraction[cell][month]),
                    f64::from(self.planet_forcing.surface_albedo()[cell]),
                ) as f32;
                if expected.to_bits()
                    != self.planet_forcing.monthly_absorbed_shortwave_w_m2()[cell][month].to_bits()
                {
                    return Err(GlobalClimateForcingError::PayloadIdentityMismatch {
                        field: "monthly_absorbed_shortwave_w_m2",
                    });
                }
            }
        }
        if self.fingerprint != self.calculate_fingerprint_impl(cancellation)? {
            return Err(GlobalClimateForcingError::FingerprintMismatch);
        }
        check_optional_cancelled(cancellation)?;
        Ok(())
    }

    fn calculate_fingerprint_impl(
        &self,
        cancellation: Option<&BuildCancellation>,
    ) -> Result<[u8; 32], GlobalClimateForcingError> {
        check_optional_cancelled(cancellation)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.global-climate-forcing.v3\0");
        hasher.update(&self.source_ref.fingerprint());
        hasher.update(&self.source_relief_fingerprint);
        hasher.update(&self.climate_spec_fingerprint);
        hasher.update(self.planet_forcing.fingerprint());
        hasher.update(&self.sea_level_m.to_bits().to_le_bytes());
        hash_f32_slice_cancellable(&mut hasher, &self.source_land_fraction, cancellation)?;
        hash_f32_slice_cancellable(&mut hasher, &self.relative_elevation_m, cancellation)?;
        hash_f32_slice_cancellable(&mut hasher, &self.ocean_depth_m, cancellation)?;
        for (index, value) in self.terrain_gradient_m_per_m.iter().enumerate() {
            poll_optional_cancelled(index, cancellation)?;
            hash_f32_slice(&mut hasher, value);
        }
        hash_f32_slice_cancellable(&mut hasher, &self.ocean_edge_permeability, cancellation)?;
        for (index, months) in self.monthly_insolation_fraction.iter().enumerate() {
            poll_optional_cancelled(index, cancellation)?;
            hash_f32_slice(&mut hasher, months);
        }
        check_optional_cancelled(cancellation)?;
        Ok(*hasher.finalize().as_bytes())
    }

    pub const fn source_ref(&self) -> SurfaceRef {
        self.source_ref
    }

    pub const fn source_relief_fingerprint(&self) -> &[u8; 32] {
        &self.source_relief_fingerprint
    }

    pub const fn climate_spec_fingerprint(&self) -> &[u8; 32] {
        &self.climate_spec_fingerprint
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub const fn sea_level_m(&self) -> f32 {
        self.sea_level_m
    }

    pub const fn planet_forcing(&self) -> &PlanetForcing {
        &self.planet_forcing
    }

    /// Exact authoritative P3 fractional land area. A value of one is full land.
    pub fn source_land_fraction(&self) -> &[f32] {
        &self.source_land_fraction
    }

    pub fn relative_elevation_m(&self) -> &[f32] {
        &self.relative_elevation_m
    }

    pub fn ocean_depth_m(&self) -> &[f32] {
        &self.ocean_depth_m
    }

    pub fn terrain_gradient_m_per_m(&self) -> &[[f32; 3]] {
        &self.terrain_gradient_m_per_m
    }

    pub fn ocean_edge_permeability(&self) -> &[f32] {
        &self.ocean_edge_permeability
    }

    pub fn monthly_insolation_fraction(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.monthly_insolation_fraction
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GlobalClimateForcingBuilder;

#[derive(Clone, Copy)]
struct ClimateTerrainInput<'a> {
    source_ref: SurfaceRef,
    source_fingerprint: [u8; 32],
    elevation_m: &'a [f32],
    surface_water_geometry: &'a SurfaceWaterGeometry,
}

impl GlobalClimateForcingBuilder {
    pub fn build(
        surface: &SphericalSurfaceSnapshot,
        relief: &PrimaryReliefSnapshot,
        climate_spec: &ClimateSpec,
        domain: &ClimateWorkDomainSnapshot,
        cancellation: &BuildCancellation,
    ) -> Result<GlobalClimateForcing, GlobalClimateForcingError> {
        let source_ref = validate_common_inputs(surface, climate_spec, domain, cancellation)?;
        if relief.surface_ref() != source_ref {
            return Err(GlobalClimateForcingError::SourceMismatch);
        }
        let terrain = ClimateTerrainInput {
            source_ref,
            source_fingerprint: relief_fingerprint_cancellable(relief, cancellation)?,
            elevation_m: relief.elevation_m(),
            surface_water_geometry: relief.surface_water_geometry(),
        };
        Self::build_from_validated_terrain(terrain, climate_spec, domain, cancellation)
    }

    /// Builds the exact production P4 forcing from a validated intermediate
    /// P5 terrain. This remains crate-private so the public P4 product boundary
    /// continues to require the authoritative P3 relief identity.
    pub(crate) fn build_for_formation_terrain(
        surface: &SphericalSurfaceSnapshot,
        terrain: &FormationTerrainFields,
        climate_spec: &ClimateSpec,
        domain: &ClimateWorkDomainSnapshot,
        cancellation: &BuildCancellation,
    ) -> Result<GlobalClimateForcing, GlobalClimateForcingError> {
        let source_ref = validate_common_inputs(surface, climate_spec, domain, cancellation)?;
        validate_formation_terrain_against_surface(surface, terrain, cancellation)?;
        let surface_water_geometry = terrain.surface_water_geometry();
        let source_fingerprint = formation_terrain_climate_fingerprint(
            source_ref,
            surface_water_geometry,
            Some(cancellation),
        )?;
        let input = ClimateTerrainInput {
            source_ref,
            source_fingerprint,
            elevation_m: terrain.current_elevation_m(),
            surface_water_geometry,
        };
        Self::build_from_validated_terrain(input, climate_spec, domain, cancellation)
    }

    fn build_from_validated_terrain(
        terrain: ClimateTerrainInput<'_>,
        climate_spec: &ClimateSpec,
        domain: &ClimateWorkDomainSnapshot,
        cancellation: &BuildCancellation,
    ) -> Result<GlobalClimateForcing, GlobalClimateForcingError> {
        let source_ref = terrain.source_ref;

        let map = domain.source_to_climate();
        let elevation_m = remap_intensive_f32_cancellable(map, terrain.elevation_m, &|| {
            cancellation.is_cancelled()
        })
        .map_err(map_remap_error)?;
        let ocean_area_fraction = terrain.surface_water_geometry.ocean_area_fraction();
        let mut source_land_fraction = Vec::with_capacity(ocean_area_fraction.len());
        for (index, &ocean_fraction) in ocean_area_fraction.iter().enumerate() {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            source_land_fraction.push(1.0 - ocean_fraction);
        }
        let land_fraction = remap_intensive_f32_cancellable(map, &source_land_fraction, &|| {
            cancellation.is_cancelled()
        })
        .map_err(map_remap_error)?;
        let sea_level_m = terrain.surface_water_geometry.sea_level_m();
        let mut source_ocean_depth = Vec::with_capacity(terrain.elevation_m.len());
        for (index, &elevation) in terrain.elevation_m.iter().enumerate() {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            source_ocean_depth.push((sea_level_m - elevation).max(0.0));
        }
        let ocean_depth_m = remap_intensive_f32_cancellable(map, &source_ocean_depth, &|| {
            cancellation.is_cancelled()
        })
        .map_err(map_remap_error)?;
        let mut relative_elevation_m = Vec::with_capacity(elevation_m.len());
        for (index, &elevation) in elevation_m.iter().enumerate() {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            relative_elevation_m.push(elevation - sea_level_m);
        }

        let grid = CubedSphereGrid::new_cancellable(
            domain.face_resolution(),
            domain.climate_surface().radius().get(),
            &|| cancellation.is_cancelled(),
        )
        .map_err(map_grid_error)?;
        if grid.fingerprint() != domain.climate_grid_fingerprint() {
            return Err(GlobalClimateForcingError::GridMismatch);
        }
        let terrain_gradient_m_per_m = CirculationOperators::new(&grid)
            .gradient_cancellable(&relative_elevation_m, cancellation)
            .map_err(map_operator_error)?;
        let work_water_geometry = build_surface_water_geometry(
            domain.climate_surface(),
            &elevation_m,
            sea_level_m,
            cancellation,
        )
        .map_err(|error| GlobalClimateForcingError::InvalidInput {
            role: "climate_surface_water_geometry",
            reason: error.to_string(),
        })?;
        let ocean_edge_permeability = work_water_geometry.wet_edge_fraction().to_vec();

        let axial_tilt_rad = f64::from(climate_spec.axial_tilt_degrees()).to_radians();
        let temperature_offset_c = f64::from(climate_spec.temperature_offset_c());
        let moisture_scale = f64::from(climate_spec.moisture_scale());
        let mut monthly_insolation_fraction = Vec::with_capacity(grid.cell_count());
        let mut equilibrium_surface_temperature_c = Vec::with_capacity(grid.cell_count());
        let mut equilibrium_air_temperature_c = Vec::with_capacity(grid.cell_count());
        let mut equilibrium_specific_humidity = Vec::with_capacity(grid.cell_count());
        let mut surface_albedo = Vec::with_capacity(grid.cell_count());
        let mut surface_moisture_availability = Vec::with_capacity(grid.cell_count());
        let mut monthly_absorbed_shortwave_w_m2 = Vec::with_capacity(grid.cell_count());
        for (index, cell) in grid.cells().iter().enumerate() {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let latitude = cell.center_unit()[2].asin();
            let land = f64::from(land_fraction[index]);
            let orography = f64::from(relative_elevation_m[index].max(0.0)) * land;
            let snow_prior = ((orography - P4_HIGHLAND_ALBEDO_RAMP_ONSET_M)
                / P4_HIGHLAND_ALBEDO_RAMP_SPAN_M)
                .clamp(0.0, 1.0);
            surface_albedo.push(
                (P4_OPEN_OCEAN_SURFACE_ALBEDO
                    + P4_SNOW_FREE_LAND_SURFACE_ALBEDO_INCREMENT * land
                    + P4_HIGHLAND_SURFACE_ALBEDO_INCREMENT * snow_prior * land)
                    as f32,
            );
            surface_moisture_availability.push(1.0_f32 - land_fraction[index]);

            let mut insolation = [0.0_f32; CLIMATE_MONTH_COUNT];
            let mut surface_temperature = [0.0_f32; CLIMATE_MONTH_COUNT];
            let mut air_temperature = [0.0_f32; CLIMATE_MONTH_COUNT];
            let mut humidity = [0.0_f32; CLIMATE_MONTH_COUNT];
            let mut absorbed_shortwave_months = [0.0_f32; CLIMATE_MONTH_COUNT];
            for month in 0..CLIMATE_MONTH_COUNT {
                let phase = std::f64::consts::TAU * (month as f64 + MONTH_PHASE_OFFSET)
                    / CLIMATE_MONTH_COUNT as f64;
                let declination = axial_tilt_rad * (-phase.cos());
                insolation[month] = daily_mean_insolation(latitude, declination) as f32;
                let absorbed_shortwave = absorbed_shortwave_w_m2(
                    f64::from(insolation[month]),
                    f64::from(surface_albedo[index]),
                );
                absorbed_shortwave_months[month] = absorbed_shortwave as f32;
                let radiative = gray_equilibrium_surface_temperature_c(absorbed_shortwave)
                    + temperature_offset_c;
                let surface_c = (radiative - CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M * orography)
                    .clamp(-90.0, 65.0);
                let air_c = surface_c.clamp(-100.0, 65.0);
                surface_temperature[month] = surface_c as f32;
                air_temperature[month] = air_c as f32;
                let saturation =
                    saturation_specific_humidity_kg_kg(f64::from(air_temperature[month]));
                humidity[month] = (saturation
                    * (moisture_scale * REFERENCE_SURFACE_RELATIVE_HUMIDITY).clamp(0.0, 1.0))
                    as f32;
            }
            monthly_insolation_fraction.push(insolation);
            equilibrium_surface_temperature_c.push(surface_temperature);
            equilibrium_air_temperature_c.push(air_temperature);
            equilibrium_specific_humidity.push(humidity);
            monthly_absorbed_shortwave_w_m2.push(absorbed_shortwave_months);
        }

        let planet_forcing = PlanetForcing::new_cancellable_with_ocean_depth(
            *grid.fingerprint(),
            elevation_m,
            land_fraction,
            ocean_depth_m.clone(),
            surface_albedo,
            surface_moisture_availability,
            monthly_absorbed_shortwave_w_m2,
            equilibrium_air_temperature_c,
            equilibrium_surface_temperature_c,
            equilibrium_specific_humidity,
            &|| cancellation.is_cancelled(),
        )
        .map_err(map_planet_forcing_error)?;
        let mut forcing = GlobalClimateForcing {
            source_ref,
            source_relief_fingerprint: terrain.source_fingerprint,
            climate_spec_fingerprint: climate_spec_fingerprint(climate_spec),
            fingerprint: [0; 32],
            sea_level_m,
            source_land_fraction,
            planet_forcing,
            relative_elevation_m,
            ocean_depth_m,
            terrain_gradient_m_per_m,
            ocean_edge_permeability,
            monthly_insolation_fraction,
        };
        forcing.fingerprint = forcing.calculate_fingerprint_impl(Some(cancellation))?;
        forcing.validate_payload_against_cancellable(domain, cancellation)?;
        Ok(forcing)
    }
}

fn validate_common_inputs(
    surface: &SphericalSurfaceSnapshot,
    climate_spec: &ClimateSpec,
    domain: &ClimateWorkDomainSnapshot,
    cancellation: &BuildCancellation,
) -> Result<SurfaceRef, GlobalClimateForcingError> {
    check_cancelled(cancellation)?;
    climate_spec
        .validate()
        .map_err(|error| GlobalClimateForcingError::InvalidInput {
            role: "climate_spec",
            reason: error.to_string(),
        })?;
    domain
        .validate_against_cancellable(surface, &|| cancellation.is_cancelled())
        .map_err(map_work_domain_error)?;
    SurfaceRef::from_validated_spherical(surface).map_err(|error| {
        GlobalClimateForcingError::InvalidInput {
            role: "surface",
            reason: error.to_string(),
        }
    })
}

fn validate_formation_terrain_against_surface(
    surface: &SphericalSurfaceSnapshot,
    terrain: &FormationTerrainFields,
    cancellation: &BuildCancellation,
) -> Result<(), GlobalClimateForcingError> {
    check_cancelled(cancellation)?;
    terrain.validate_against_surface(surface).map_err(|error| {
        GlobalClimateForcingError::InvalidInput {
            role: "formation_terrain",
            reason: error.to_string(),
        }
    })?;
    let expected = surface.cells().len();
    let found = terrain.current_elevation_m().len();
    if found != expected {
        return Err(GlobalClimateForcingError::FieldLengthMismatch {
            field: "formation_terrain.current_elevation_m",
            found,
            expected,
        });
    }

    check_cancelled(cancellation)
}

fn daily_mean_insolation(latitude: f64, declination: f64) -> f64 {
    let argument = (-latitude.tan() * declination.tan()).clamp(-1.0, 1.0);
    let sunset_hour_angle = argument.acos();
    ((sunset_hour_angle * latitude.sin() * declination.sin()
        + latitude.cos() * declination.cos() * sunset_hour_angle.sin())
        / std::f64::consts::PI)
        .max(0.0)
}

fn relief_fingerprint_cancellable(
    relief: &PrimaryReliefSnapshot,
    cancellation: &BuildCancellation,
) -> Result<[u8; 32], GlobalClimateForcingError> {
    relief_fingerprint_impl(relief, Some(cancellation))
}

fn relief_fingerprint_impl(
    relief: &PrimaryReliefSnapshot,
    cancellation: Option<&BuildCancellation>,
) -> Result<[u8; 32], GlobalClimateForcingError> {
    climate_terrain_fingerprint_impl(
        relief.surface_ref(),
        relief.surface_water_geometry(),
        cancellation,
    )
}

fn formation_terrain_climate_fingerprint(
    surface_ref: SurfaceRef,
    surface_water_geometry: &SurfaceWaterGeometry,
    cancellation: Option<&BuildCancellation>,
) -> Result<[u8; 32], GlobalClimateForcingError> {
    climate_terrain_fingerprint_impl(surface_ref, surface_water_geometry, cancellation)
}

fn climate_terrain_fingerprint_impl(
    surface_ref: SurfaceRef,
    surface_water_geometry: &SurfaceWaterGeometry,
    cancellation: Option<&BuildCancellation>,
) -> Result<[u8; 32], GlobalClimateForcingError> {
    check_optional_cancelled(cancellation)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.climate-terrain-input.v2\0");
    hasher.update(&surface_ref.fingerprint());
    hasher.update(surface_water_geometry.fingerprint());
    check_optional_cancelled(cancellation)?;
    Ok(*hasher.finalize().as_bytes())
}

fn climate_spec_fingerprint(spec: &ClimateSpec) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.global-climate-spec.v1\0");
    hasher.update(&spec.schema_version.to_le_bytes());
    hasher.update(&spec.axial_tilt_centideg.to_le_bytes());
    hasher.update(&spec.temperature_offset_deci_c.to_le_bytes());
    hasher.update(&spec.moisture_scale_permille.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn hash_f32_slice(hasher: &mut blake3::Hasher, values: &[f32]) {
    for value in values {
        hasher.update(&value.to_bits().to_le_bytes());
    }
}

fn hash_f32_slice_cancellable(
    hasher: &mut blake3::Hasher,
    values: &[f32],
    cancellation: Option<&BuildCancellation>,
) -> Result<(), GlobalClimateForcingError> {
    for (index, value) in values.iter().enumerate() {
        poll_optional_cancelled(index, cancellation)?;
        hasher.update(&value.to_bits().to_le_bytes());
    }
    Ok(())
}

fn map_work_domain_error(error: ClimateWorkDomainValidationError) -> GlobalClimateForcingError {
    if error == ClimateWorkDomainValidationError::Cancelled {
        GlobalClimateForcingError::Cancelled
    } else {
        GlobalClimateForcingError::WorkDomain(error)
    }
}

fn map_remap_error(error: ConservativeRemapError) -> GlobalClimateForcingError {
    if error == ConservativeRemapError::Cancelled {
        GlobalClimateForcingError::Cancelled
    } else {
        GlobalClimateForcingError::Remap(error)
    }
}

fn map_operator_error(error: CirculationOperatorError) -> GlobalClimateForcingError {
    if error == CirculationOperatorError::Cancelled {
        GlobalClimateForcingError::Cancelled
    } else {
        GlobalClimateForcingError::Operator(error)
    }
}

fn map_grid_error(error: CubedSphereGridError) -> GlobalClimateForcingError {
    if error == CubedSphereGridError::Cancelled {
        GlobalClimateForcingError::Cancelled
    } else {
        GlobalClimateForcingError::CubedSphere(error)
    }
}

fn map_planet_forcing_error(error: ForcingError) -> GlobalClimateForcingError {
    if error == ForcingError::Cancelled {
        GlobalClimateForcingError::Cancelled
    } else {
        GlobalClimateForcingError::InvalidForcing {
            reason: error.to_string(),
        }
    }
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), GlobalClimateForcingError> {
    if cancellation.is_cancelled() {
        Err(GlobalClimateForcingError::Cancelled)
    } else {
        Ok(())
    }
}

fn poll_optional_cancelled(
    index: usize,
    cancellation: Option<&BuildCancellation>,
) -> Result<(), GlobalClimateForcingError> {
    if index % 256 == 0 {
        check_optional_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_optional_cancelled(
    cancellation: Option<&BuildCancellation>,
) -> Result<(), GlobalClimateForcingError> {
    if cancellation.is_some_and(BuildCancellation::is_cancelled) {
        Err(GlobalClimateForcingError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum GlobalClimateForcingError {
    #[error("global climate forcing build was cancelled")]
    Cancelled,
    #[error("invalid {role} input: {reason}")]
    InvalidInput { role: &'static str, reason: String },
    #[error(transparent)]
    Relief(#[from] PrimaryReliefValidationError),
    #[error(transparent)]
    WorkDomain(#[from] ClimateWorkDomainValidationError),
    #[error(transparent)]
    CubedSphere(#[from] CubedSphereGridError),
    #[error(transparent)]
    Remap(#[from] ConservativeRemapError),
    #[error(transparent)]
    Operator(#[from] CirculationOperatorError),
    #[error("invalid shared planet forcing: {reason}")]
    InvalidForcing { reason: String },
    #[error("P3 relief or forcing source does not match the climate work domain")]
    SourceMismatch,
    #[error("climate work-grid fingerprint does not reconstruct exactly")]
    GridMismatch,
    #[error("global forcing field {field} disagrees with the shared planet forcing payload")]
    PayloadIdentityMismatch { field: &'static str },
    #[error("forcing field {field} has {found} values, expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        found: usize,
        expected: usize,
    },
    #[error("forcing {field}[{index}]={found} is outside {minimum}..={maximum}")]
    ValueOutOfRange {
        field: &'static str,
        index: usize,
        found: f32,
        minimum: f32,
        maximum: f32,
    },
    #[error("global climate forcing fingerprint mismatch")]
    FingerprintMismatch,
}

#[cfg(test)]
mod formation_tests {
    use std::sync::OnceLock;

    use super::*;
    use crate::generators::natural::{
        solve_physical_sea_level, ClimateWorkDomainBuilder, GlobalCirculationGenerator,
    };
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        constraint_status, land_fraction_constraint_tolerance, ClimateModelProfile,
        FormationElevationComponents, FormationSedimentFields, FormationTerrainFields,
        NaturalQualityProfile, PrimaryReliefSnapshot, ReliefSpec,
        FORMATION_TERRAIN_FIELDS_SCHEMA_V4, PRIMARY_RELIEF_SCHEMA_V3,
    };
    use crate::world::{Meters, SphericalSpaceSpec};

    struct Fixture {
        surface: SphericalSurfaceSnapshot,
        relief: PrimaryReliefSnapshot,
        domain: ClimateWorkDomainSnapshot,
    }

    fn fixture() -> &'static Fixture {
        static FIXTURE: OnceLock<Fixture> = OnceLock::new();
        FIXTURE.get_or_init(|| {
            let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
                radius: Meters::new(6_371_000.0).unwrap(),
                target_cell_count: 42,
            })
            .unwrap();
            let count = surface.cells().len();
            let primary = vec![-1_000.0; count];
            let zero = vec![0.0; count];
            let areas = surface
                .cells()
                .iter()
                .map(|cell| cell.area.get())
                .collect::<Vec<_>>();
            let water_inventory = areas.iter().sum::<f64>() * 1_000.0;
            let water = solve_physical_sea_level(&surface, &primary, water_inventory).unwrap();
            let physical = water
                .geometry()
                .global_land_area_fraction(&surface)
                .unwrap();
            let tolerance = land_fraction_constraint_tolerance(&surface).unwrap();
            let relief = PrimaryReliefSnapshot::new(
                PRIMARY_RELIEF_SCHEMA_V3,
                SurfaceRef::for_spherical(&surface),
                primary.clone(),
                zero.clone(),
                zero.clone(),
                zero,
                primary,
                water_inventory,
                water.geometry().clone(),
                ReliefSpec::default().target_land_fraction,
                physical,
                tolerance,
                constraint_status(
                    ReliefSpec::default().target_land_fraction,
                    physical,
                    tolerance,
                ),
            )
            .unwrap();
            let domain = ClimateWorkDomainBuilder::build(
                &surface,
                NaturalQualityProfile::Draft,
                &BuildCancellation::new(),
            )
            .unwrap();
            Fixture {
                surface,
                relief,
                domain,
            }
        })
    }

    fn candidate(
        surface: &SphericalSurfaceSnapshot,
        relief: &PrimaryReliefSnapshot,
        changed_cell: Option<usize>,
    ) -> FormationTerrainFields {
        let mut primary = relief.elevation_m().to_vec();
        if let Some(cell) = changed_cell {
            primary[cell] += 250.0;
        }
        terrain_from_primary(surface, primary, relief.water_inventory_m3())
    }

    fn terrain_from_primary(
        surface: &SphericalSurfaceSnapshot,
        primary: Vec<f32>,
        water_inventory_m3: f64,
    ) -> FormationTerrainFields {
        let count = surface.cells().len();
        assert_eq!(primary.len(), count);
        let zero = vec![0.0; count];
        let components = FormationElevationComponents::new(
            primary.clone(),
            zero.clone(),
            zero.clone(),
            zero.clone(),
            zero.clone(),
            zero.clone(),
            zero.clone(),
            zero.clone(),
            zero,
            primary.clone(),
        )
        .unwrap();
        let water = solve_physical_sea_level(surface, &primary, water_inventory_m3).unwrap();
        FormationTerrainFields::new(
            FORMATION_TERRAIN_FIELDS_SCHEMA_V4,
            components,
            water.into_geometry(),
            water_inventory_m3,
            FormationSedimentFields::new(
                vec![0.0; count],
                vec![[0.0; 5]; count],
                vec![0.0; count],
                vec![0.0; count],
                vec![0.0; count],
                vec![0.0; count],
                vec![0.0; count],
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn formation_terrain_reuses_exact_p4_forcing_and_changes_checkpoint_causally() {
        let fixture = fixture();
        let cancellation = BuildCancellation::new();
        let baseline = GlobalClimateForcingBuilder::build(
            &fixture.surface,
            &fixture.relief,
            &ClimateSpec::default(),
            &fixture.domain,
            &cancellation,
        )
        .unwrap();
        let unchanged = GlobalClimateForcingBuilder::build_for_formation_terrain(
            &fixture.surface,
            &candidate(&fixture.surface, &fixture.relief, None),
            &ClimateSpec::default(),
            &fixture.domain,
            &cancellation,
        )
        .unwrap();
        assert_eq!(unchanged, baseline);

        let changed = GlobalClimateForcingBuilder::build_for_formation_terrain(
            &fixture.surface,
            &candidate(&fixture.surface, &fixture.relief, Some(0)),
            &ClimateSpec::default(),
            &fixture.domain,
            &cancellation,
        )
        .unwrap();
        let changed_repeated = GlobalClimateForcingBuilder::build_for_formation_terrain(
            &fixture.surface,
            &candidate(&fixture.surface, &fixture.relief, Some(0)),
            &ClimateSpec::default(),
            &fixture.domain,
            &BuildCancellation::new(),
        )
        .unwrap();
        assert_eq!(changed_repeated, changed);
        assert_ne!(changed.fingerprint(), baseline.fingerprint());
        assert!(changed.validate_relief_identity(&fixture.relief).is_err());

        let baseline_climate = GlobalCirculationGenerator::generate(
            &fixture.surface,
            &fixture.domain,
            &baseline,
            ClimateModelProfile::C2LayeredV1,
            &cancellation,
        )
        .unwrap();
        let changed_climate = GlobalCirculationGenerator::generate(
            &fixture.surface,
            &fixture.domain,
            &changed,
            ClimateModelProfile::C2LayeredV1,
            &cancellation,
        )
        .unwrap();
        assert_ne!(
            baseline_climate.checkpoint().forcing_fingerprint(),
            changed_climate.checkpoint().forcing_fingerprint()
        );
        assert_ne!(
            baseline_climate.checkpoint().input_fingerprint(),
            changed_climate.checkpoint().input_fingerprint()
        );
        assert_eq!(changed_climate.profile(), ClimateModelProfile::C2LayeredV1);
    }

    #[test]
    fn formation_terrain_forcing_rejects_wrong_allocation_and_cancellation() {
        let fixture = fixture();
        let other = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 92,
        })
        .unwrap();
        let other_inventory = other
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .sum::<f64>()
            * 1_000.0;
        let wrong =
            terrain_from_primary(&other, vec![-1_000.0; other.cells().len()], other_inventory);
        assert!(matches!(
            GlobalClimateForcingBuilder::build_for_formation_terrain(
                &fixture.surface,
                &wrong,
                &ClimateSpec::default(),
                &fixture.domain,
                &BuildCancellation::new(),
            ),
            Err(GlobalClimateForcingError::InvalidInput {
                role: "formation_terrain",
                ..
            })
        ));

        let cancellation = BuildCancellation::new();
        cancellation.cancel();
        assert_eq!(
            GlobalClimateForcingBuilder::build_for_formation_terrain(
                &fixture.surface,
                &candidate(&fixture.surface, &fixture.relief, None),
                &ClimateSpec::default(),
                &fixture.domain,
                &cancellation,
            ),
            Err(GlobalClimateForcingError::Cancelled)
        );

        let terrain = candidate(&fixture.surface, &fixture.relief, Some(0));
        let cancellation = BuildCancellation::new();
        let (observed_before_request, result) = std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                GlobalClimateForcingBuilder::build_for_formation_terrain(
                    &fixture.surface,
                    &terrain,
                    &ClimateSpec::default(),
                    &fixture.domain,
                    &cancellation,
                )
            });
            while cancellation.observation_count() < 8 && !worker.is_finished() {
                std::hint::spin_loop();
            }
            let observed = cancellation.observation_count();
            cancellation.cancel();
            (observed, worker.join().unwrap())
        });
        assert!(observed_before_request >= 8);
        assert_eq!(result, Err(GlobalClimateForcingError::Cancelled));
    }
}
