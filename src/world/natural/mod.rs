//! Contracts for the current-slice natural world.

mod fields;
mod geologic_spec;
mod geology;
mod mantle;
mod relief;
mod spec;
mod tectonics;

pub use fields::{
    bedrock_kind_field_id, boundary_kind_field_id, boundary_strength_field_id,
    crust_base_elevation_field_id, crust_kind_field_id, crust_thickness_field_id,
    elevation_field_id, erosion_resistance_field_id, fracture_intensity_field_id,
    geothermal_potential_field_id, land_ocean_field_id, mantle_heat_flow_field_id,
    metallic_mineral_potential_field_id, natural_field_registry, plate_id_field_id,
    plate_velocity_field_id, regional_offset_field_id, relative_permeability_field_id,
    sedimentary_basin_potential_field_id, tectonic_offset_field_id, volcanic_influence_field_id,
    volcanic_offset_field_id, NaturalFieldDisplayCache, NaturalFieldRegistryError,
};
pub use geologic_spec::{
    GeologicSpec, GeologicSpecError, MantleActivity, GEOLOGIC_SPEC_SCHEMA_V1, MAX_HOTSPOT_COUNT,
};
pub use geology::{
    BedrockKind, BedrockKindField, GeologicSnapshot, GeologicValidationError,
    GEOLOGIC_SNAPSHOT_SCHEMA_V1,
};
pub use mantle::{
    Hotspot, MantleSnapshot, MantleValidationError, HEAT_FLOW_MAX_MW_M2, HEAT_FLOW_MIN_MW_M2,
    MANTLE_SNAPSHOT_SCHEMA_V1, MAX_HOTSPOT_STRENGTH_PERMILLE, MIN_HOTSPOT_STRENGTH_PERMILLE,
};
pub use relief::{
    ElevationField, LandOceanField, LandOceanKind, ReliefSnapshot, ReliefValidationError,
    COMPONENT_IDENTITY_TOLERANCE_M, CRUST_BASE_ELEVATION_MAX_M, CRUST_BASE_ELEVATION_MIN_M,
    ELEVATION_MAX_M, ELEVATION_MIN_M, REGIONAL_OFFSET_MAX_M, REGIONAL_OFFSET_MIN_M,
    RELIEF_SCHEMA_V1, RELIEF_SCHEMA_V2, TECTONIC_OFFSET_MAX_M, TECTONIC_OFFSET_MIN_M,
    VOLCANIC_OFFSET_MAX_M, VOLCANIC_OFFSET_MIN_M,
};
pub use spec::{
    NaturalSpecError, TectonicActivity, TectonicSpec, MAX_CONTINENTAL_CRUST_FRACTION,
    MAX_PLATE_COUNT, MIN_CONTINENTAL_CRUST_FRACTION, MIN_PLATE_COUNT, TECTONIC_SPEC_SCHEMA_V1,
};
pub use tectonics::{
    BoundaryKind, BoundaryRecord, BoundarySegment, CrustKind, CrustKindField, Plate, PlateIdField,
    PlateVelocity, TectonicSnapshot, TectonicValidationError, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
    CONTINENTAL_CRUST_MIN_THICKNESS_KM, MAX_PLATE_VELOCITY_MM_PER_YEAR,
    OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM, TECTONIC_SNAPSHOT_SCHEMA_V1,
};
