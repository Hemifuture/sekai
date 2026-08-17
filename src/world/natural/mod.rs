//! Contracts for the current-slice natural world.

mod circulation;
mod climate;
mod climate_spec;
mod fields;
mod formation;
mod geologic_spec;
mod geology;
mod hydro_erosion;
mod hydro_erosion_spec;
mod hydrology;
mod mantle;
mod profile;
mod quality;
mod relief;
mod relief_spec;
mod spec;
mod spherical_climate;
mod spherical_geology;
mod spherical_hydro_erosion;
mod spherical_hydrology;
mod spherical_mantle;
mod spherical_relief;
mod spherical_surface_process;
mod spherical_tectonics;
mod surface_process;
mod tectonics;

pub use circulation::{
    CirculationSnapshot, CirculationSnapshotError, CirculationSolveStats, CirculationSolverId,
    CirculationSpec, CirculationSpecError, ForcingError, PlanetForcing, CIRCULATION_SCHEMA_V1,
    MAX_CUBED_SPHERE_FACE_RESOLUTION,
};
pub use climate::{
    ClimateValidationError, MonthlyScalarField, MonthlyVector3Field, MonthlyVectorField,
    PreliminaryClimateSnapshot, AIR_TEMPERATURE_MAX_C, AIR_TEMPERATURE_MIN_C,
    ANNUAL_PRECIPITATION_MAX_MM, CLIMATE_MONTH_COUNT, CLIMATE_SUMMARY_IDENTITY_TOLERANCE,
    MONTHLY_PRECIPITATION_MAX_MM, PRELIMINARY_CLIMATE_SCHEMA_V1, PRELIMINARY_CLIMATE_SCHEMA_V2,
    TEMPERATURE_SEASONALITY_MAX_C, WIND_COMPONENT_MAX_M_S,
};
pub use climate_spec::{
    ClimateSpec, ClimateSpecError, CLIMATE_SPEC_SCHEMA_V1, MAX_AXIAL_TILT_CENTIDEG,
    MAX_LATITUDE_CENTIDEG, MAX_MOISTURE_SCALE_PERMILLE, MAX_TEMPERATURE_OFFSET_DECI_C,
    MIN_LATITUDE_CENTIDEG, MIN_LATITUDE_SPAN_CENTIDEG, MIN_MOISTURE_SCALE_PERMILLE,
    MIN_TEMPERATURE_OFFSET_DECI_C,
};
pub use fields::{
    annual_local_runoff_mm_field_id, bedrock_kind_field_id, boundary_kind_field_id,
    boundary_strength_field_id, crust_base_elevation_field_id, crust_kind_field_id,
    crust_thickness_field_id, drainage_area_km2_field_id, elevation_field_id,
    erosion_resistance_field_id, fluvial_erosion_depth_m_field_id, fracture_intensity_field_id,
    geothermal_potential_field_id, lake_depth_m_field_id, land_ocean_field_id,
    latitude_degrees_field_id, mantle_heat_flow_field_id, maritime_influence_field_id,
    mean_annual_discharge_m3_s_field_id, metallic_mineral_potential_field_id,
    natural_field_registry, plate_id_field_id, plate_velocity_field_id,
    preliminary_annual_precipitation_mm_field_id, preliminary_mean_air_temperature_c_field_id,
    preliminary_prevailing_wind_m_s_field_id, preliminary_temperature_seasonality_c_field_id,
    regional_offset_field_id, relative_permeability_field_id,
    sediment_deposition_thickness_m_field_id, sedimentary_basin_potential_field_id,
    spherical_natural_field_registry, strahler_stream_order_field_id, surface_elevation_m_field_id,
    surface_water_kind_field_id, tectonic_offset_field_id, volcanic_influence_field_id,
    volcanic_offset_field_id, NaturalFieldDisplayCache, NaturalFieldRegistryError,
};
pub use formation::{
    MantleFormationBias, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    WorldFormationPreset, WorldFormationSpec, WorldFormationSpecError,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1, WORLD_FORMATION_SPEC_SCHEMA_V1,
};
pub use geologic_spec::{
    GeologicSpec, GeologicSpecError, MantleActivity, GEOLOGIC_SPEC_SCHEMA_V1, MAX_HOTSPOT_COUNT,
};
pub use geology::{
    BedrockKind, BedrockKindField, GeologicSnapshot, GeologicValidationError,
    GEOLOGIC_SNAPSHOT_SCHEMA_V1, GEOLOGIC_SNAPSHOT_SCHEMA_V2,
};
pub use hydro_erosion::{
    HydroErosionSnapshot, HydroErosionValidationError, HYDRO_EROSION_SNAPSHOT_SCHEMA_V1,
    HYDRO_EROSION_SNAPSHOT_SCHEMA_V2, RUNOFF_IDENTITY_TOLERANCE_MM,
};
pub use hydro_erosion_spec::{
    HydroErosionSpec, HydroErosionSpecError, HYDRO_EROSION_SPEC_SCHEMA_V1,
    MAX_EROSION_STRENGTH_PERMILLE, MAX_LAKE_DEPTH_CM, MAX_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S,
    MIN_LAKE_DEPTH_CM, MIN_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S,
};
pub use hydrology::{
    BasinOutletKind, DrainageBasin, HydrologySnapshot, HydrologyValidationError, Lake,
    RiverSegment, RiverSegmentKind, StrahlerOrderField, SurfaceWaterField, SurfaceWaterKind,
    CLIMATOLOGICAL_YEAR_SECONDS, DISCHARGE_ACCUMULATION_ABSOLUTE_TOLERANCE_M3_S,
    DRAINAGE_AREA_ABSOLUTE_TOLERANCE_KM2, HYDROLOGY_SCHEMA_V1, HYDROLOGY_SCHEMA_V2,
    HYDROLOGY_SUMMARY_ABSOLUTE_TOLERANCE, HYDROLOGY_SUMMARY_RELATIVE_TOLERANCE, MAX_LAKE_DEPTH_M,
    MAX_STRAHLER_ORDER, SECONDS_PER_CLIMATOLOGICAL_MONTH,
};
pub use mantle::{
    Hotspot, MantleSnapshot, MantleValidationError, HEAT_FLOW_MAX_MW_M2, HEAT_FLOW_MIN_MW_M2,
    MANTLE_SNAPSHOT_SCHEMA_V1, MAX_HOTSPOT_STRENGTH_PERMILLE, MIN_HOTSPOT_STRENGTH_PERMILLE,
};
pub use profile::{
    NaturalProfileError, NaturalQualityProfile, NaturalResolutionPlan,
    NATURAL_RESOLUTION_PLAN_SCHEMA_V1,
};
pub use quality::{
    NaturalQualityReport, NaturalQualityValidationError, QualityBounds, QualityMetric,
    QualityMetricId, QualityMetricStatus, NATURAL_QUALITY_REPORT_SCHEMA_V1,
};
pub use relief::{
    ElevationField, LandOceanField, LandOceanKind, ReliefSnapshot, ReliefValidationError,
    COMPONENT_IDENTITY_TOLERANCE_M, CRUST_BASE_ELEVATION_MAX_M, CRUST_BASE_ELEVATION_MIN_M,
    ELEVATION_MAX_M, ELEVATION_MIN_M, REGIONAL_OFFSET_MAX_M, REGIONAL_OFFSET_MIN_M,
    RELIEF_SCHEMA_V1, RELIEF_SCHEMA_V2, RELIEF_SCHEMA_V3, RELIEF_SCHEMA_V4, TECTONIC_OFFSET_MAX_M,
    TECTONIC_OFFSET_MIN_M, VOLCANIC_OFFSET_MAX_M, VOLCANIC_OFFSET_MIN_M,
};
pub use relief_spec::{
    ReliefSpec, ReliefSpecError, MAX_TARGET_LAND_FRACTION, MIN_TARGET_LAND_FRACTION,
    RELIEF_SPEC_SCHEMA_V1,
};
pub use spec::{
    NaturalSpecError, TectonicActivity, TectonicSpec, MAX_CONTINENTAL_CRUST_FRACTION,
    MAX_PLATE_COUNT, MIN_CONTINENTAL_CRUST_FRACTION, MIN_PLATE_COUNT, TECTONIC_SPEC_SCHEMA_V1,
};
pub use spherical_climate::{
    SphericalClimateValidationError, SphericalPreliminaryClimateSnapshot,
    SPHERICAL_LATITUDE_IDENTITY_TOLERANCE_DEGREES, SPHERICAL_WIND_TANGENCY_TOLERANCE_M_S,
};
pub use spherical_geology::{SphericalGeologicSnapshot, SphericalGeologicValidationError};
pub use spherical_hydro_erosion::{
    SphericalHydroErosionSnapshot, SphericalHydroErosionValidationError,
};
pub use spherical_hydrology::{SphericalHydrologySnapshot, SphericalHydrologyValidationError};
pub use spherical_mantle::{
    SphericalMantleSnapshot, SphericalMantleValidationError, MANTLE_SNAPSHOT_SCHEMA_V2,
};
pub use spherical_relief::{SphericalReliefSnapshot, SphericalReliefValidationError};
pub use spherical_surface_process::{
    SphericalSurfaceProcessSnapshot, SphericalSurfaceProcessValidationError,
};
pub(crate) use spherical_tectonics::classify_spherical_boundary_kinematics;
pub use spherical_tectonics::{
    SphericalBoundarySegment, SphericalCrustState, SphericalOrogenyKind, SphericalPlate,
    SphericalPlateRotation, SphericalTectonicSnapshot, SphericalTectonicValidationError,
    CONTINENTAL_CRUST_AGE_SENTINEL_MYR, MAX_CRUST_AGE_MYR,
    MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR, MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR,
    NO_OROGENY_AGE_SENTINEL_MYR, TECTONIC_SNAPSHOT_SCHEMA_V3,
};
pub use surface_process::{
    SurfaceProcessSnapshot, SurfaceProcessValidationError, MAX_DEPOSITION_THICKNESS_M,
    MAX_EROSION_DEPTH_M, SEDIMENT_VOLUME_ABSOLUTE_TOLERANCE_M3, SEDIMENT_VOLUME_RELATIVE_TOLERANCE,
    SURFACE_IDENTITY_TOLERANCE_M, SURFACE_PROCESS_SCHEMA_V1, SURFACE_PROCESS_SCHEMA_V2,
};
pub(crate) use tectonics::{
    classify_boundary_kinematics, BoundaryClassification, BoundaryKinematics,
};
pub use tectonics::{
    BoundaryKind, BoundaryRecord, BoundarySegment, CrustKind, CrustKindField, Plate, PlateIdField,
    PlateVelocity, TectonicSnapshot, TectonicValidationError, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
    CONTINENTAL_CRUST_MIN_THICKNESS_KM, MAX_PLATE_VELOCITY_MM_PER_YEAR,
    OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM, TECTONIC_SNAPSHOT_SCHEMA_V1,
};
