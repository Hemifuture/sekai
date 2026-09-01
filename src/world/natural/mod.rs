//! Contracts for the current-slice natural world.

mod circulation;
mod climate;
mod climate_spec;
mod evolved_tectonics;
mod fields;
mod formation;
mod formation_bundle;
mod geologic_spec;
mod geology;
mod global_circulation;
mod hydro_erosion;
mod hydro_erosion_spec;
mod hydrology;
mod hypsometry;
mod mantle;
mod primary_relief;
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
mod surface_formation;
mod surface_process;
mod surface_water_geometry;
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
pub use evolved_tectonics::{
    CrustMaterialResidual, CrustMaterialTotals, EvolvedTectonicSnapshot,
    EvolvedTectonicValidationError, SphericalCrustMaterialState, SphericalTectonicForcingState,
    SphericalTectonicLineageBudget, SphericalTectonicMaterialBudget,
    SphericalTectonicMaterialProcesses, TectonicMaterialAmount,
    EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1, MATERIAL_THICKNESS_TOLERANCE_KM,
    MAX_TECTONIC_AUTHORITY_RELATIVE_BUDGET_ERROR, MAX_TECTONIC_CONTROL_RELATIVE_BUDGET_ERROR,
    MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR,
};
pub use fields::{
    annual_local_runoff_mm_field_id, bedrock_kind_field_id, boundary_kind_field_id,
    boundary_strength_field_id, circulation_annual_evaporation_mm_field_id,
    circulation_annual_precipitation_mm_field_id,
    circulation_mean_absorbed_shortwave_w_m2_field_id, circulation_mean_air_temperature_c_field_id,
    circulation_mean_outgoing_longwave_w_m2_field_id, circulation_prevailing_wind_m_s_field_id,
    circulation_surface_albedo_field_id, coastal_deposition_m_field_id,
    coastal_deposition_rate_m_per_year_field_id, coastal_erosion_m_field_id,
    coastal_erosion_rate_m_per_year_field_id, crust_base_elevation_field_id, crust_kind_field_id,
    crust_thickness_field_id, drainage_area_km2_field_id, elevation_field_id,
    erosion_resistance_field_id, fluvial_erosion_depth_m_field_id,
    fluvial_erosion_rate_m_per_year_field_id, fracture_intensity_field_id,
    geothermal_potential_field_id, hillslope_deposition_m_field_id,
    hillslope_deposition_rate_m_per_year_field_id, hillslope_erosion_m_field_id,
    hillslope_erosion_rate_m_per_year_field_id, isostatic_response_m_field_id,
    isostatic_response_rate_m_per_year_field_id, lake_depth_m_field_id, land_ocean_field_id,
    latitude_degrees_field_id, mantle_heat_flow_field_id, maritime_influence_field_id,
    mean_annual_discharge_m3_s_field_id, metallic_mineral_potential_field_id,
    natural_field_registry, ocean_age_myr_field_id, plate_id_field_id, plate_velocity_field_id,
    preliminary_annual_precipitation_mm_field_id, preliminary_mean_air_temperature_c_field_id,
    preliminary_prevailing_wind_m_s_field_id, preliminary_temperature_seasonality_c_field_id,
    primary_elevation_m_field_id, regional_offset_field_id, relative_permeability_field_id,
    routed_sediment_deposition_m_field_id, routed_sediment_deposition_rate_m_per_year_field_id,
    sediment_deposition_thickness_m_field_id, sedimentary_basin_potential_field_id,
    spherical_formation_field_registry, spherical_natural_field_registry,
    strahler_stream_order_field_id, surface_elevation_m_field_id, surface_water_kind_field_id,
    tectonic_displacement_m_field_id, tectonic_displacement_rate_m_per_year_field_id,
    tectonic_offset_field_id, volcanic_influence_field_id, volcanic_offset_field_id,
    NaturalFieldDisplayCache, NaturalFieldRegistryError,
};
pub use formation::{
    MantleFormationBias, ResolvedFormationTimeline, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, WorldFormationPreset, WorldFormationSpec,
    WorldFormationSpecError, CORTIAL_FORMATION_STEP_DURATION_KYR,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1, SEKAI_REFERENCE_FORMATION_STEP_COUNT,
    WORLD_FORMATION_SPEC_SCHEMA_V1,
};
pub(crate) use formation_bundle::NaturalFormationBundleParts;
pub use formation_bundle::{
    NaturalFormationBundle, NaturalFormationBundleValidationError,
    NATURAL_FORMATION_BUNDLE_SCHEMA_V1,
};
pub use geologic_spec::{
    GeologicSpec, GeologicSpecError, MantleActivity, GEOLOGIC_SPEC_SCHEMA_V1, MAX_HOTSPOT_COUNT,
};
pub use geology::{
    BedrockKind, BedrockKindField, GeologicSnapshot, GeologicValidationError,
    GEOLOGIC_SNAPSHOT_SCHEMA_V1, GEOLOGIC_SNAPSHOT_SCHEMA_V2,
};
pub(crate) use global_circulation::p4_thermodynamic_constants_fingerprint;
pub use global_circulation::{
    absorbed_shortwave_w_m2, bulk_surface_evaporation_kg_m2_s, climatological_annual_total_mm,
    climatological_monthly_mean, expected_global_circulation_dense_state_bytes,
    gray_equilibrium_surface_temperature_c, large_scale_condensation_kg_m2_s,
    latent_heat_flux_w_m2_from_evaporation_mm_day, lcl_adjusted_orographic_condensation_kg_m2_s,
    linearized_outgoing_longwave_w_m2, neutral_surface_air_specific_humidity_kg_kg,
    planetary_albedo_from_surface, raw_orographic_condensation_kg_m2_s,
    saturation_specific_humidity_kg_kg, water_cycle_relative_imbalance, ClimateBudgetReport,
    ClimateCapabilityAvailability, ClimateCapabilityError, ClimateCapabilityId,
    ClimateCapabilitySet, ClimateCheckpoint, ClimateCheckpointError, ClimateLayerExchangeSpec,
    ClimateLayerLayout, ClimateLayerLayoutError, ClimateLayerRole, ClimateLayerSpec,
    ClimateModelProfile, ClimateQuantizationId, ClimateRemapReport, ClimateReportError,
    ClimateSolveReport, ClimateWorkDomainSnapshot, ClimateWorkDomainValidationError,
    GlobalCirculationFields, GlobalCirculationSnapshot, GlobalCirculationValidationError,
    ProductionIntegratorId, BULK_MOISTURE_TRANSFER_COEFFICIENT,
    CERES_EBAF_ABSORBED_SHORTWAVE_GLOBAL_MEAN_W_M2, CERES_EBAF_INCOMING_SHORTWAVE_GLOBAL_MEAN_W_M2,
    CERES_EBAF_OUTGOING_LONGWAVE_GLOBAL_MEAN_W_M2, CERES_EBAF_REFLECTED_SHORTWAVE_GLOBAL_MEAN_W_M2,
    CERES_EBAF_SURFACE_UP_LONGWAVE_GLOBAL_MEAN_W_M2, CERES_EBAF_TOA_NET_RADIATION_GLOBAL_MEAN_W_M2,
    CLIMATE_CHECKPOINT_SCHEMA_V2, CLIMATE_LAYER_LAYOUT_SCHEMA_V1,
    CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M, CLIMATE_WORK_DOMAIN_SCHEMA_V1,
    EARTH_ATMOSPHERIC_SHORTWAVE_REFLECTANCE, EARTH_CALIBRATION_SURFACE_ALBEDO_GLOBAL_MEAN,
    EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN,
    EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE,
    EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY, EARTH_GRAY_GREENHOUSE_OFFSET_K,
    EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2, GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX,
    GLOBAL_CIRCULATION_DENSE_STATE_BYTES_MAX, GLOBAL_CIRCULATION_ENERGY_RELATIVE_ERROR_MAX,
    GLOBAL_CIRCULATION_FORMATION_CYCLES_MAX, GLOBAL_CIRCULATION_FORMATION_RESIDUAL_MAX,
    GLOBAL_CIRCULATION_MACRO_STEP_SECONDS, GLOBAL_CIRCULATION_RADIATIVE_FLUX_MAX_W_M2,
    GLOBAL_CIRCULATION_SCHEMA_V2, GLOBAL_CIRCULATION_TANGENCY_TOLERANCE_M_S,
    GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2, GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX,
    P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K, P4_HIGHLAND_ALBEDO_RAMP_ONSET_M,
    P4_HIGHLAND_ALBEDO_RAMP_SPAN_M, P4_HIGHLAND_SURFACE_ALBEDO_INCREMENT,
    P4_LARGE_SCALE_CONDENSATION_RELATIVE_HUMIDITY, P4_LARGE_SCALE_CONDENSATION_RELAXATION_SECONDS,
    P4_LOWER_LAYER_REFERENCE_PRESSURE_PA, P4_MAX_SPECIFIC_HUMIDITY_KG_KG,
    P4_OPEN_OCEAN_SURFACE_ALBEDO, P4_REFERENCE_AIR_DENSITY_KG_M3,
    P4_SNOW_FREE_LAND_SURFACE_ALBEDO_INCREMENT, REFERENCE_SURFACE_RELATIVE_HUMIDITY,
    STANDARD_GRAVITY_M_S2, STEFAN_BOLTZMANN_CONSTANT_W_M2_K4,
    STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2, STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2,
    WATER_VAPORIZATION_LATENT_HEAT_J_KG, WILD_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2,
    WILD_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2,
};
#[cfg(test)]
pub(crate) use global_circulation::{
    global_circulation_owner_inventory, global_circulation_tendency_cell_bytes,
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
    MAX_STRAHLER_ORDER, MEAN_SOLAR_DAY_SECONDS, SECONDS_PER_CLIMATOLOGICAL_MONTH,
};
pub use hypsometry::{
    hypsometric_mean, hypsometric_quantile, hypsometric_share_below, hypsometric_total_area,
    sort_hypsometric_samples,
};
pub use mantle::{
    Hotspot, MantleSnapshot, MantleValidationError, HEAT_FLOW_MAX_MW_M2, HEAT_FLOW_MIN_MW_M2,
    MANTLE_SNAPSHOT_SCHEMA_V1, MAX_HOTSPOT_STRENGTH_PERMILLE, MIN_HOTSPOT_STRENGTH_PERMILLE,
};
pub use primary_relief::{
    constraint_status, effective_crust_density_kg_m3, land_fraction_constraint_tolerance,
    scaled_earth_ocean_inventory_m3, sediment_source_for_bedrock, GeologicSubstrateSnapshot,
    GeologicSubstrateValidationError, LandFractionConstraintStatus, PrimaryReliefSnapshot,
    PrimaryReliefValidationError, SedimentSourceKind, SedimentSourceKindField, WaterVolumeSolution,
    WaterVolumeSolveError, CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M, CONTINENTAL_CRUST_DENSITY_KG_M3,
    CRUST_BASE_ELEVATION_MAX_M, CRUST_DENSITY_MAX_KG_M3, CRUST_DENSITY_MIN_KG_M3,
    EARTH_OCEANIC_SEDIMENT_MEAN_THICKNESS_M, EARTH_OCEAN_CRUST_MEAN_AGE_MYR, EARTH_OCEAN_VOLUME_M3,
    EARTH_WATER_REFERENCE_RADIUS_M, GEOLOGIC_SUBSTRATE_SCHEMA_V1,
    MIN_LAND_FRACTION_CONSTRAINT_TOLERANCE, OCEANIC_CRUST_DENSITY_KG_M3,
    OCEANIC_SEDIMENT_DENSITY_KG_M3, OCEAN_WATER_DENSITY_KG_M3, PASSIVE_MARGIN_OFFSET_ABS_MAX_M,
    PRIMARY_RELIEF_SCHEMA_V3, WATER_VOLUME_RELATIVE_TOLERANCE,
};
pub(crate) use primary_relief::{
    continental_airy_elevation_exact_m, water_volume_relative_error,
    CRUST_BASE_ELEVATION_MAX_EXACT_M, PRIMARY_RELIEF_AIRY_MANTLE_DENSITY_KG_M3,
    PRIMARY_RELIEF_CONTINENTAL_REFERENCE_THICKNESS_KM,
    PRIMARY_RELIEF_OCEANIC_REFERENCE_THICKNESS_KM,
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
    COMPONENT_IDENTITY_TOLERANCE_M, CRUST_BASE_ELEVATION_MIN_M, ELEVATION_MAX_M, ELEVATION_MIN_M,
    REGIONAL_OFFSET_MAX_M, REGIONAL_OFFSET_MIN_M, RELIEF_SCHEMA_V1, RELIEF_SCHEMA_V2,
    RELIEF_SCHEMA_V3, RELIEF_SCHEMA_V4, TECTONIC_OFFSET_MAX_M, TECTONIC_OFFSET_MIN_M,
    VOLCANIC_OFFSET_MAX_M, VOLCANIC_OFFSET_MIN_M,
};
pub use relief_spec::{
    ReliefSpec, ReliefSpecError, SeaLevelPolicy, MAX_TARGET_LAND_FRACTION,
    MAX_WATER_INVENTORY_RATIO, MIN_TARGET_LAND_FRACTION, MIN_WATER_INVENTORY_RATIO,
    OCEAN_FLOOR_EXPOSURE_HINT_FRACTION, RELIEF_SPEC_SCHEMA_V2, WATER_INVENTORY_RATIO_ADVISORY_MAX,
    WATER_INVENTORY_RATIO_ADVISORY_MIN,
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
    asthenosphere_mobility, SphericalBoundarySegment, SphericalCrustState, SphericalOrogenyKind,
    SphericalPlate, SphericalPlateRotation, SphericalTectonicSnapshot,
    SphericalTectonicValidationError, CLOOS_OCEANIC_NEGATIVE_BUOYANCY_AGE_MYR,
    CONTINENTAL_CRUST_AGE_SENTINEL_MYR, MAXIMUM_PLATE_AREA_FRACTION, MAX_CRUST_AGE_MYR,
    MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR, MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR,
    NO_OROGENY_AGE_SENTINEL_MYR, PLATE_COLLISION_RESISTANCE_PER_M,
    PLATE_CONTINENT_BASAL_DRAG_PER_M2, PLATE_LOCKED_MARGIN_RESISTANCE_PER_M,
    PLATE_OCEAN_BASAL_DRAG_PER_M2, PLATE_RIDGE_PUSH_FORCE_PER_M, PLATE_SLAB_PULL_FORCE_PER_M,
    PLATE_SLAB_SUCTION_FORCE_PER_M, TECTONIC_SNAPSHOT_SCHEMA_V3,
};
pub use surface_formation::{
    expected_surface_formation_dense_state_bytes, formation_annual_precipitation_mm,
    formation_elevation_from_components, formation_monthly_precipitation_mm,
    surface_formation_model_fingerprint, surface_formation_state_fingerprint,
    FormationElevationComponents, FormationEvolutionReport, FormationProcessRates,
    FormationResiduals, FormationSedimentFields, FormationTerrainFields,
    NaturalSurfaceFormationSnapshot, SedimentBudgetReport, SurfaceFormationCapabilityAvailability,
    SurfaceFormationCapabilityId, SurfaceFormationCapabilitySet, SurfaceFormationCheckpoint,
    SurfaceFormationModelId, SurfaceFormationUpstreamFingerprints, SurfaceFormationValidationError,
    FORMATION_AIRY_MANTLE_DENSITY_KG_M3, FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
    FORMATION_COASTAL_COVER_SHIELD_M, FORMATION_COASTAL_CURRENT_REFERENCE_M_S,
    FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR, FORMATION_COASTAL_WIND_REFERENCE_M_S,
    FORMATION_ENDORHEIC_RESIDENCE_YEARS, FORMATION_FLOODPLAIN_ACCOMMODATION_M,
    FORMATION_HILLSLOPE_CRITICAL_SLOPE, FORMATION_HILLSLOPE_DENOMINATOR_MIN,
    FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR, FORMATION_HILLSLOPE_ERODIBILITY_BASE,
    FORMATION_HILLSLOPE_ERODIBILITY_RANGE, FORMATION_HILLSLOPE_FRACTURE_BASE,
    FORMATION_HILLSLOPE_FRACTURE_RANGE, FORMATION_HILLSLOPE_PRECIPITATION_FACTOR_MAX,
    FORMATION_HILLSLOPE_PRECIPITATION_REFERENCE_MM, FORMATION_HILLSLOPE_WEATHERING_BASE,
    FORMATION_HILLSLOPE_WEATHERING_RANGE, FORMATION_MINIMUM_LAKE_DEPTH_M,
    FORMATION_RUNOFF_MIN_FRACTION, FORMATION_RUNOFF_PERMEABILITY_RANGE,
    FORMATION_SHELF_BREAK_DEPTH_M, FORMATION_STREAM_POWER_AREA_EXPONENT,
    FORMATION_STREAM_POWER_ERODIBILITY_BASE, FORMATION_STREAM_POWER_ERODIBILITY_RANGE,
    FORMATION_STREAM_POWER_REFERENCE_ERODIBILITY_PER_YEAR,
    FORMATION_STREAM_POWER_RUNOFF_REFERENCE_MM, FORMATION_STREAM_POWER_SLOPE_EXPONENT,
    FORMATION_STREAM_POWER_SLOPE_THRESHOLD, FORMATION_TERRAIN_FIELDS_SCHEMA_V4,
    NATURAL_SURFACE_FORMATION_SCHEMA_V5, SEDIMENT_BUDGET_RELATIVE_ERROR_MAX,
    SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX, SEDIMENT_PROVENANCE_SOURCE_COUNT,
    SURFACE_FORMATION_CHECKPOINT_SCHEMA_V2, SURFACE_FORMATION_DENSE_STATE_BYTES_MAX,
    SURFACE_FORMATION_HORIZON_YEARS,
};
pub(crate) use surface_formation::{
    formation_relative_flux_imbalance, FORMATION_SEDIMENT_DEPOSITION_COEFFICIENT,
};
pub use surface_process::{
    SurfaceProcessSnapshot, SurfaceProcessValidationError, MAX_DEPOSITION_THICKNESS_M,
    MAX_EROSION_DEPTH_M, SEDIMENT_VOLUME_ABSOLUTE_TOLERANCE_M3, SEDIMENT_VOLUME_RELATIVE_TOLERANCE,
    SURFACE_IDENTITY_TOLERANCE_M, SURFACE_PROCESS_SCHEMA_V1, SURFACE_PROCESS_SCHEMA_V2,
};
pub(crate) use surface_water_geometry::surface_elevation_fingerprint;
pub use surface_water_geometry::{
    SurfaceWaterGeometry, SurfaceWaterGeometryValidationError, SURFACE_WATER_GEOMETRY_SCHEMA_V1,
};
pub(crate) use tectonics::{
    classify_boundary_kinematics, BoundaryClassification, BoundaryKinematics,
};
pub use tectonics::{
    BoundaryKind, BoundaryRecord, BoundarySegment, CrustKind, CrustKindField, Plate, PlateIdField,
    PlateVelocity, TectonicSnapshot, TectonicValidationError, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
    CONTINENTAL_CRUST_MIN_THICKNESS_KM, CRUST1_PLATFORM_THICKNESS_QUANTILES_KM,
    MAX_PLATE_VELOCITY_MM_PER_YEAR, OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM,
    TECTONIC_SNAPSHOT_SCHEMA_V1,
};
