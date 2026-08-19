use crate::view::{DisplayRangeMode, FieldPayloadRef};
use crate::world::fields::{FieldId, FieldRegistry, ValueRange};
use crate::world::natural::{
    annual_local_runoff_mm_field_id, bedrock_kind_field_id, boundary_kind_field_id,
    boundary_strength_field_id, crust_base_elevation_field_id, crust_kind_field_id,
    crust_thickness_field_id, drainage_area_km2_field_id, elevation_field_id,
    erosion_resistance_field_id, fluvial_erosion_depth_m_field_id, fracture_intensity_field_id,
    geothermal_potential_field_id, lake_depth_m_field_id, land_ocean_field_id,
    latitude_degrees_field_id, mantle_heat_flow_field_id, maritime_influence_field_id,
    mean_annual_discharge_m3_s_field_id, metallic_mineral_potential_field_id, plate_id_field_id,
    plate_velocity_field_id, preliminary_annual_precipitation_mm_field_id,
    preliminary_mean_air_temperature_c_field_id, preliminary_prevailing_wind_m_s_field_id,
    preliminary_temperature_seasonality_c_field_id, regional_offset_field_id,
    relative_permeability_field_id, sediment_deposition_thickness_m_field_id,
    sedimentary_basin_potential_field_id, strahler_stream_order_field_id,
    surface_elevation_m_field_id, surface_water_kind_field_id, tectonic_offset_field_id,
    volcanic_influence_field_id, volcanic_offset_field_id, GeologicSnapshot, HydroErosionSnapshot,
    MantleSnapshot, PreliminaryClimateSnapshot, ReliefSnapshot, SphericalGeologicSnapshot,
    SphericalHydroErosionSnapshot, SphericalMantleSnapshot, SphericalPreliminaryClimateSnapshot,
    SphericalReliefSnapshot, SphericalTectonicSnapshot, TectonicSnapshot,
};

/// Borrowed natural-field values independent of any presentation mesh.
pub(super) struct NaturalFieldPayloadBundle<'a> {
    plate_id: &'a [u32],
    crust_kind: &'a [u32],
    crust_thickness_km: &'a [f32],
    plate_velocity: &'a [[f32; 2]],
    boundary_kind: &'a [u32],
    boundary_strength: &'a [f32],
    crust_base_elevation_m: &'a [f32],
    tectonic_offset_m: &'a [f32],
    regional_offset_m: &'a [f32],
    elevation_m: &'a [f32],
    land_ocean: &'a [u32],
    mantle_heat_flow_mw_m2: &'a [f32],
    volcanic_influence: &'a [f32],
    volcanic_offset_m: &'a [f32],
    bedrock_kind: &'a [u32],
    fracture_intensity: &'a [f32],
    erosion_resistance: &'a [f32],
    relative_permeability: &'a [f32],
    metallic_mineral_potential: &'a [f32],
    geothermal_potential: &'a [f32],
    sedimentary_basin_potential: &'a [f32],
    latitude_degrees: &'a [f32],
    maritime_influence: &'a [f32],
    preliminary_prevailing_wind_m_s: &'a [[f32; 2]],
    preliminary_mean_air_temperature_c: &'a [f32],
    preliminary_temperature_seasonality_c: &'a [f32],
    preliminary_annual_precipitation_mm: &'a [f32],
    surface_elevation_m: &'a [f32],
    fluvial_erosion_depth_m: &'a [f32],
    sediment_deposition_thickness_m: &'a [f32],
    surface_water_kind: &'a [u32],
    lake_depth_m: &'a [f32],
    annual_local_runoff_mm: &'a [f32],
    mean_annual_discharge_m3_s: &'a [f32],
    drainage_area_km2: &'a [f32],
    strahler_stream_order: &'a [u32],
}

impl<'a> NaturalFieldPayloadBundle<'a> {
    /// Borrows every legacy-planar natural field without copying its arrays.
    pub(super) fn from_legacy_planar(
        tectonic: &'a TectonicSnapshot,
        mantle: &'a MantleSnapshot,
        relief: &'a ReliefSnapshot,
        geology: &'a GeologicSnapshot,
        climate: &'a PreliminaryClimateSnapshot,
        hydro_erosion: &'a HydroErosionSnapshot,
        plate_velocity_cm_per_year: &'a [[f32; 2]],
        boundary_kind: &'a [u32],
        boundary_strength: &'a [f32],
    ) -> Self {
        Self {
            plate_id: tectonic.cell_plates().raw_values(),
            crust_kind: tectonic.crust_kinds().raw_values(),
            crust_thickness_km: tectonic.crust_thickness_km(),
            plate_velocity: plate_velocity_cm_per_year,
            boundary_kind,
            boundary_strength,
            crust_base_elevation_m: relief.crust_base_elevation_m().values(),
            tectonic_offset_m: relief.tectonic_offset_m().values(),
            regional_offset_m: relief.regional_offset_m().values(),
            elevation_m: relief.elevation_m().values(),
            land_ocean: relief.land_ocean().raw_values(),
            mantle_heat_flow_mw_m2: mantle.heat_flow_mw_m2(),
            volcanic_influence: mantle.volcanic_influence(),
            volcanic_offset_m: relief.volcanic_offset_m().values(),
            bedrock_kind: geology.bedrock_kinds().raw_values(),
            fracture_intensity: geology.fracture_intensity(),
            erosion_resistance: geology.erosion_resistance(),
            relative_permeability: geology.relative_permeability(),
            metallic_mineral_potential: geology.metallic_mineral_potential(),
            geothermal_potential: geology.geothermal_potential(),
            sedimentary_basin_potential: geology.sedimentary_basin_potential(),
            latitude_degrees: climate.latitude_degrees(),
            maritime_influence: climate.maritime_influence(),
            preliminary_prevailing_wind_m_s: climate.prevailing_wind_m_s(),
            preliminary_mean_air_temperature_c: climate.mean_annual_air_temperature_c(),
            preliminary_temperature_seasonality_c: climate.temperature_seasonality_c(),
            preliminary_annual_precipitation_mm: climate.annual_precipitation_mm(),
            surface_elevation_m: hydro_erosion.surface().surface_elevation_m().values(),
            fluvial_erosion_depth_m: hydro_erosion.surface().erosion_depth_m(),
            sediment_deposition_thickness_m: hydro_erosion.surface().deposition_thickness_m(),
            surface_water_kind: hydro_erosion.hydrology().surface_water().raw_values(),
            lake_depth_m: hydro_erosion.hydrology().lake_depth_m(),
            annual_local_runoff_mm: hydro_erosion.hydrology().annual_local_runoff_mm(),
            mean_annual_discharge_m3_s: hydro_erosion.hydrology().mean_annual_discharge_m3_s(),
            drainage_area_km2: hydro_erosion.hydrology().drainage_area_km2(),
            strahler_stream_order: hydro_erosion.hydrology().strahler_order().raw_values(),
        }
    }

    /// Borrows every spherical natural field and its disposable local vectors.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn from_spherical(
        tectonic: &'a SphericalTectonicSnapshot,
        mantle: &'a SphericalMantleSnapshot,
        relief: &'a SphericalReliefSnapshot,
        geology: &'a SphericalGeologicSnapshot,
        climate: &'a SphericalPreliminaryClimateSnapshot,
        hydro_erosion: &'a SphericalHydroErosionSnapshot,
        plate_velocity_cm_per_year: &'a [[f32; 2]],
        prevailing_wind_m_s: &'a [[f32; 2]],
        boundary_kind: &'a [u32],
        boundary_strength: &'a [f32],
    ) -> Self {
        Self {
            plate_id: tectonic.cell_plates().raw_values(),
            crust_kind: tectonic.crust_kinds().raw_values(),
            crust_thickness_km: tectonic.crust_thickness_km(),
            plate_velocity: plate_velocity_cm_per_year,
            boundary_kind,
            boundary_strength,
            crust_base_elevation_m: relief.crust_base_elevation_m().values(),
            tectonic_offset_m: relief.tectonic_offset_m().values(),
            regional_offset_m: relief.regional_offset_m().values(),
            elevation_m: relief.elevation_m().values(),
            land_ocean: relief.land_ocean().raw_values(),
            mantle_heat_flow_mw_m2: mantle.heat_flow_mw_m2(),
            volcanic_influence: mantle.volcanic_influence(),
            volcanic_offset_m: relief.volcanic_offset_m().values(),
            bedrock_kind: geology.bedrock_kinds().raw_values(),
            fracture_intensity: geology.fracture_intensity(),
            erosion_resistance: geology.erosion_resistance(),
            relative_permeability: geology.relative_permeability(),
            metallic_mineral_potential: geology.metallic_mineral_potential(),
            geothermal_potential: geology.geothermal_potential(),
            sedimentary_basin_potential: geology.sedimentary_basin_potential(),
            latitude_degrees: climate.latitude_degrees(),
            maritime_influence: climate.maritime_influence(),
            preliminary_prevailing_wind_m_s: prevailing_wind_m_s,
            preliminary_mean_air_temperature_c: climate.mean_annual_air_temperature_c(),
            preliminary_temperature_seasonality_c: climate.temperature_seasonality_c(),
            preliminary_annual_precipitation_mm: climate.annual_precipitation_mm(),
            surface_elevation_m: hydro_erosion.surface().surface_elevation_m().values(),
            fluvial_erosion_depth_m: hydro_erosion.surface().erosion_depth_m(),
            sediment_deposition_thickness_m: hydro_erosion.surface().deposition_thickness_m(),
            surface_water_kind: hydro_erosion.hydrology().surface_water().raw_values(),
            lake_depth_m: hydro_erosion.hydrology().lake_depth_m(),
            annual_local_runoff_mm: hydro_erosion.hydrology().annual_local_runoff_mm(),
            mean_annual_discharge_m3_s: hydro_erosion.hydrology().mean_annual_discharge_m3_s(),
            drainage_area_km2: hydro_erosion.hydrology().drainage_area_km2(),
            strahler_stream_order: hydro_erosion.hydrology().strahler_order().raw_values(),
        }
    }

    /// Returns the sole stable mapping from natural Field IDs to borrowed values.
    pub(super) fn payloads(&self) -> Vec<(FieldId, FieldPayloadRef<'a>)> {
        vec![
            (
                plate_id_field_id(),
                FieldPayloadRef::CategoryU32(self.plate_id),
            ),
            (
                crust_kind_field_id(),
                FieldPayloadRef::CategoryU32(self.crust_kind),
            ),
            (
                crust_thickness_field_id(),
                FieldPayloadRef::ScalarF32(self.crust_thickness_km),
            ),
            (
                plate_velocity_field_id(),
                FieldPayloadRef::Vector2F32(self.plate_velocity),
            ),
            (
                boundary_kind_field_id(),
                FieldPayloadRef::CategoryU32(self.boundary_kind),
            ),
            (
                boundary_strength_field_id(),
                FieldPayloadRef::ScalarF32(self.boundary_strength),
            ),
            (
                crust_base_elevation_field_id(),
                FieldPayloadRef::ScalarF32(self.crust_base_elevation_m),
            ),
            (
                tectonic_offset_field_id(),
                FieldPayloadRef::ScalarF32(self.tectonic_offset_m),
            ),
            (
                regional_offset_field_id(),
                FieldPayloadRef::ScalarF32(self.regional_offset_m),
            ),
            (
                elevation_field_id(),
                FieldPayloadRef::ScalarF32(self.elevation_m),
            ),
            (
                land_ocean_field_id(),
                FieldPayloadRef::CategoryU32(self.land_ocean),
            ),
            (
                mantle_heat_flow_field_id(),
                FieldPayloadRef::ScalarF32(self.mantle_heat_flow_mw_m2),
            ),
            (
                volcanic_influence_field_id(),
                FieldPayloadRef::ScalarF32(self.volcanic_influence),
            ),
            (
                volcanic_offset_field_id(),
                FieldPayloadRef::ScalarF32(self.volcanic_offset_m),
            ),
            (
                bedrock_kind_field_id(),
                FieldPayloadRef::CategoryU32(self.bedrock_kind),
            ),
            (
                fracture_intensity_field_id(),
                FieldPayloadRef::ScalarF32(self.fracture_intensity),
            ),
            (
                erosion_resistance_field_id(),
                FieldPayloadRef::ScalarF32(self.erosion_resistance),
            ),
            (
                relative_permeability_field_id(),
                FieldPayloadRef::ScalarF32(self.relative_permeability),
            ),
            (
                metallic_mineral_potential_field_id(),
                FieldPayloadRef::ScalarF32(self.metallic_mineral_potential),
            ),
            (
                geothermal_potential_field_id(),
                FieldPayloadRef::ScalarF32(self.geothermal_potential),
            ),
            (
                sedimentary_basin_potential_field_id(),
                FieldPayloadRef::ScalarF32(self.sedimentary_basin_potential),
            ),
            (
                latitude_degrees_field_id(),
                FieldPayloadRef::ScalarF32(self.latitude_degrees),
            ),
            (
                maritime_influence_field_id(),
                FieldPayloadRef::ScalarF32(self.maritime_influence),
            ),
            (
                preliminary_prevailing_wind_m_s_field_id(),
                FieldPayloadRef::Vector2F32(self.preliminary_prevailing_wind_m_s),
            ),
            (
                preliminary_mean_air_temperature_c_field_id(),
                FieldPayloadRef::ScalarF32(self.preliminary_mean_air_temperature_c),
            ),
            (
                preliminary_temperature_seasonality_c_field_id(),
                FieldPayloadRef::ScalarF32(self.preliminary_temperature_seasonality_c),
            ),
            (
                preliminary_annual_precipitation_mm_field_id(),
                FieldPayloadRef::ScalarF32(self.preliminary_annual_precipitation_mm),
            ),
            (
                surface_elevation_m_field_id(),
                FieldPayloadRef::ScalarF32(self.surface_elevation_m),
            ),
            (
                fluvial_erosion_depth_m_field_id(),
                FieldPayloadRef::ScalarF32(self.fluvial_erosion_depth_m),
            ),
            (
                sediment_deposition_thickness_m_field_id(),
                FieldPayloadRef::ScalarF32(self.sediment_deposition_thickness_m),
            ),
            (
                surface_water_kind_field_id(),
                FieldPayloadRef::CategoryU32(self.surface_water_kind),
            ),
            (
                lake_depth_m_field_id(),
                FieldPayloadRef::ScalarF32(self.lake_depth_m),
            ),
            (
                annual_local_runoff_mm_field_id(),
                FieldPayloadRef::ScalarF32(self.annual_local_runoff_mm),
            ),
            (
                mean_annual_discharge_m3_s_field_id(),
                FieldPayloadRef::ScalarF32(self.mean_annual_discharge_m3_s),
            ),
            (
                drainage_area_km2_field_id(),
                FieldPayloadRef::ScalarF32(self.drainage_area_km2),
            ),
            (
                strahler_stream_order_field_id(),
                FieldPayloadRef::CategoryU32(self.strahler_stream_order),
            ),
        ]
    }
}

/// Returns the shared natural-field range preference for a current surface.
///
/// `elevation_display_radius_m` is the document-precomputed percentile radius
/// so per-frame reconciliation never re-sorts the elevation field.
pub(super) fn natural_preferred_range(
    registry: &FieldRegistry,
    sea_level_m: f32,
    elevation_display_radius_m: Option<f32>,
    field: &FieldId,
) -> Option<DisplayRangeMode> {
    if [
        annual_local_runoff_mm_field_id(),
        drainage_area_km2_field_id(),
        fluvial_erosion_depth_m_field_id(),
        lake_depth_m_field_id(),
        mean_annual_discharge_m3_s_field_id(),
        sediment_deposition_thickness_m_field_id(),
    ]
    .contains(field)
    {
        return Some(DisplayRangeMode::Data);
    }
    (field == &surface_elevation_m_field_id() || field == &elevation_field_id()).then_some(())?;
    registry.get(field)?;
    let radius = elevation_display_radius_m?;
    ValueRange::new(sea_level_m - radius, sea_level_m + radius)
        .ok()
        .map(DisplayRangeMode::Manual)
}

/// Percentile of the absolute deviation from sea level used as the symmetric
/// elevation display radius.
const ELEVATION_DISPLAY_RADIUS_PERCENTILE: f64 = 0.98;

/// Returns a symmetric hypsometric display radius around sea level.
///
/// The radius is the 98th percentile of `|elevation − sea level|` instead of
/// the maximum, so one extreme trench or peak cannot compress the rest of the
/// world into the palette midpoint; values beyond the radius clamp to the
/// palette ends. The radius stays symmetric because the hypsometric palette
/// crosses from water to land exactly at its midpoint.
pub(super) fn elevation_display_radius_m(
    sea_level_m: f32,
    surface_elevation_m: &[f32],
) -> Option<f32> {
    if surface_elevation_m.is_empty() {
        return None;
    }
    let mut deviations: Vec<f32> = surface_elevation_m
        .iter()
        .map(|value| (value - sea_level_m).abs())
        .collect();
    deviations.sort_unstable_by(f32::total_cmp);
    let index =
        ((deviations.len() - 1) as f64 * ELEVATION_DISPLAY_RADIUS_PERCENTILE).round() as usize;
    let radius = deviations[index.min(deviations.len() - 1)];
    radius.is_finite().then_some(radius.max(1.0))
}
