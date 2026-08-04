use std::sync::Arc;

use thiserror::Error;

use super::field_document::AppFieldDocument;
use crate::engine::{BuildReport, DiagnosticSeverity};
use crate::generators::natural::{
    GeologicArtifact, HydroErosionArtifact, MantleArtifact, PreliminaryClimateArtifact,
    ReliefArtifact, ResolvedWorldFormationArtifact, TectonicArtifact,
};
use crate::generators::spatial::SpatialArtifact;
use crate::view::{
    DisplayPrepareError, DisplayRangeMode, FieldCatalog, FieldPayloadRef, FieldViewError,
    MeshCompleteness, OwnedViewDiagnostic, PreparedCellMesh, ViewDiagnosticSeverity,
};
use crate::world::fields::{FieldId, FieldRegistry, ValueRange};
use crate::world::natural::{
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
    strahler_stream_order_field_id, surface_elevation_m_field_id, surface_water_kind_field_id,
    tectonic_offset_field_id, volcanic_influence_field_id, volcanic_offset_field_id,
    ClimateValidationError, GeologicValidationError, HydroErosionValidationError,
    MantleValidationError, NaturalFieldDisplayCache, NaturalFieldRegistryError,
    ReliefValidationError, TectonicValidationError, WorldFormationSpecError,
};
use crate::world::spatial::SpatialValidationError;

/// Immutable formal natural-world document used by the application display boundary.
pub(super) struct LegacyPlanarNaturalFieldDocument {
    pub(super) spatial: Arc<SpatialArtifact>,
    pub(super) formation: Arc<ResolvedWorldFormationArtifact>,
    pub(super) tectonic: Arc<TectonicArtifact>,
    pub(super) mantle: Arc<MantleArtifact>,
    pub(super) relief: Arc<ReliefArtifact>,
    pub(super) geology: Arc<GeologicArtifact>,
    pub(super) climate: Arc<PreliminaryClimateArtifact>,
    pub(super) hydro_erosion: Arc<HydroErosionArtifact>,
    registry: FieldRegistry,
    mesh: Arc<PreparedCellMesh>,
    diagnostics: Vec<OwnedViewDiagnostic>,
    display_cache: NaturalFieldDisplayCache,
}

impl LegacyPlanarNaturalFieldDocument {
    pub(super) fn build(
        spatial: Arc<SpatialArtifact>,
        formation: Arc<ResolvedWorldFormationArtifact>,
        tectonic: Arc<TectonicArtifact>,
        mantle: Arc<MantleArtifact>,
        relief: Arc<ReliefArtifact>,
        geology: Arc<GeologicArtifact>,
        climate: Arc<PreliminaryClimateArtifact>,
        hydro_erosion: Arc<HydroErosionArtifact>,
        report: &BuildReport,
    ) -> Result<Self, NaturalDisplayError> {
        spatial.snapshot().validate()?;
        formation.formation().validate()?;
        tectonic.snapshot().validate_against(spatial.snapshot())?;
        mantle.snapshot().validate_against(spatial.snapshot())?;
        relief.snapshot().validate_against(spatial.snapshot())?;
        geology.snapshot().validate_against(
            spatial.snapshot(),
            tectonic.snapshot(),
            mantle.snapshot(),
            relief.snapshot(),
        )?;
        climate
            .snapshot()
            .validate_against(spatial.snapshot(), relief.snapshot())?;
        hydro_erosion
            .snapshot()
            .validate_against_validated_spatial(
                spatial.snapshot(),
                relief.snapshot(),
                geology.snapshot(),
                climate.snapshot(),
            )?;
        let plate_count = u16::try_from(tectonic.snapshot().plates().len())
            .map_err(|_| NaturalDisplayError::PlateCountOverflow)?;
        let registry = natural_field_registry(plate_count)?;
        let mesh = Arc::new(PreparedCellMesh::build(
            spatial.snapshot(),
            MeshCompleteness::RequireAll,
        )?);
        let diagnostics = report
            .diagnostics()
            .iter()
            .map(|diagnostic| OwnedViewDiagnostic {
                severity: match diagnostic.severity() {
                    DiagnosticSeverity::Info => ViewDiagnosticSeverity::Info,
                    DiagnosticSeverity::Warning => ViewDiagnosticSeverity::Warning,
                    DiagnosticSeverity::Error => ViewDiagnosticSeverity::Error,
                },
                code: diagnostic.code().to_owned(),
                field_id: diagnostic.context().field_id.clone(),
                cell_id: diagnostic.context().cell_id,
                message: diagnostic.message().to_owned(),
            })
            .collect();
        let display_cache = NaturalFieldDisplayCache::new(tectonic.snapshot());
        let document = Self {
            spatial,
            formation,
            tectonic,
            mantle,
            relief,
            geology,
            climate,
            hydro_erosion,
            registry,
            mesh,
            diagnostics,
            display_cache,
        };
        document.catalog()?;
        Ok(document)
    }

    fn payloads(&self) -> Vec<(FieldId, FieldPayloadRef<'_>)> {
        vec![
            (
                plate_id_field_id(),
                FieldPayloadRef::CategoryU32(self.tectonic.snapshot().cell_plates().raw_values()),
            ),
            (
                crust_kind_field_id(),
                FieldPayloadRef::CategoryU32(self.tectonic.snapshot().crust_kinds().raw_values()),
            ),
            (
                crust_thickness_field_id(),
                FieldPayloadRef::ScalarF32(self.tectonic.snapshot().crust_thickness_km()),
            ),
            (
                plate_velocity_field_id(),
                FieldPayloadRef::Vector2F32(self.display_cache.plate_velocity_cm_per_year()),
            ),
            (
                boundary_kind_field_id(),
                FieldPayloadRef::CategoryU32(self.display_cache.boundary_kind()),
            ),
            (
                boundary_strength_field_id(),
                FieldPayloadRef::ScalarF32(self.display_cache.boundary_strength()),
            ),
            (
                crust_base_elevation_field_id(),
                FieldPayloadRef::ScalarF32(
                    self.relief.snapshot().crust_base_elevation_m().values(),
                ),
            ),
            (
                tectonic_offset_field_id(),
                FieldPayloadRef::ScalarF32(self.relief.snapshot().tectonic_offset_m().values()),
            ),
            (
                regional_offset_field_id(),
                FieldPayloadRef::ScalarF32(self.relief.snapshot().regional_offset_m().values()),
            ),
            (
                elevation_field_id(),
                FieldPayloadRef::ScalarF32(self.relief.snapshot().elevation_m().values()),
            ),
            (
                land_ocean_field_id(),
                FieldPayloadRef::CategoryU32(self.relief.snapshot().land_ocean().raw_values()),
            ),
            (
                mantle_heat_flow_field_id(),
                FieldPayloadRef::ScalarF32(self.mantle.snapshot().heat_flow_mw_m2()),
            ),
            (
                volcanic_influence_field_id(),
                FieldPayloadRef::ScalarF32(self.mantle.snapshot().volcanic_influence()),
            ),
            (
                volcanic_offset_field_id(),
                FieldPayloadRef::ScalarF32(self.relief.snapshot().volcanic_offset_m().values()),
            ),
            (
                bedrock_kind_field_id(),
                FieldPayloadRef::CategoryU32(self.geology.snapshot().bedrock_kinds().raw_values()),
            ),
            (
                fracture_intensity_field_id(),
                FieldPayloadRef::ScalarF32(self.geology.snapshot().fracture_intensity()),
            ),
            (
                erosion_resistance_field_id(),
                FieldPayloadRef::ScalarF32(self.geology.snapshot().erosion_resistance()),
            ),
            (
                relative_permeability_field_id(),
                FieldPayloadRef::ScalarF32(self.geology.snapshot().relative_permeability()),
            ),
            (
                metallic_mineral_potential_field_id(),
                FieldPayloadRef::ScalarF32(self.geology.snapshot().metallic_mineral_potential()),
            ),
            (
                geothermal_potential_field_id(),
                FieldPayloadRef::ScalarF32(self.geology.snapshot().geothermal_potential()),
            ),
            (
                sedimentary_basin_potential_field_id(),
                FieldPayloadRef::ScalarF32(self.geology.snapshot().sedimentary_basin_potential()),
            ),
            (
                latitude_degrees_field_id(),
                FieldPayloadRef::ScalarF32(self.climate.snapshot().latitude_degrees()),
            ),
            (
                maritime_influence_field_id(),
                FieldPayloadRef::ScalarF32(self.climate.snapshot().maritime_influence()),
            ),
            (
                preliminary_prevailing_wind_m_s_field_id(),
                FieldPayloadRef::Vector2F32(self.climate.snapshot().prevailing_wind_m_s()),
            ),
            (
                preliminary_mean_air_temperature_c_field_id(),
                FieldPayloadRef::ScalarF32(self.climate.snapshot().mean_annual_air_temperature_c()),
            ),
            (
                preliminary_temperature_seasonality_c_field_id(),
                FieldPayloadRef::ScalarF32(self.climate.snapshot().temperature_seasonality_c()),
            ),
            (
                preliminary_annual_precipitation_mm_field_id(),
                FieldPayloadRef::ScalarF32(self.climate.snapshot().annual_precipitation_mm()),
            ),
            (
                surface_elevation_m_field_id(),
                FieldPayloadRef::ScalarF32(
                    self.hydro_erosion
                        .snapshot()
                        .surface()
                        .surface_elevation_m()
                        .values(),
                ),
            ),
            (
                fluvial_erosion_depth_m_field_id(),
                FieldPayloadRef::ScalarF32(
                    self.hydro_erosion.snapshot().surface().erosion_depth_m(),
                ),
            ),
            (
                sediment_deposition_thickness_m_field_id(),
                FieldPayloadRef::ScalarF32(
                    self.hydro_erosion
                        .snapshot()
                        .surface()
                        .deposition_thickness_m(),
                ),
            ),
            (
                surface_water_kind_field_id(),
                FieldPayloadRef::CategoryU32(
                    self.hydro_erosion
                        .snapshot()
                        .hydrology()
                        .surface_water()
                        .raw_values(),
                ),
            ),
            (
                lake_depth_m_field_id(),
                FieldPayloadRef::ScalarF32(
                    self.hydro_erosion.snapshot().hydrology().lake_depth_m(),
                ),
            ),
            (
                annual_local_runoff_mm_field_id(),
                FieldPayloadRef::ScalarF32(
                    self.hydro_erosion
                        .snapshot()
                        .hydrology()
                        .annual_local_runoff_mm(),
                ),
            ),
            (
                mean_annual_discharge_m3_s_field_id(),
                FieldPayloadRef::ScalarF32(
                    self.hydro_erosion
                        .snapshot()
                        .hydrology()
                        .mean_annual_discharge_m3_s(),
                ),
            ),
            (
                drainage_area_km2_field_id(),
                FieldPayloadRef::ScalarF32(
                    self.hydro_erosion
                        .snapshot()
                        .hydrology()
                        .drainage_area_km2(),
                ),
            ),
            (
                strahler_stream_order_field_id(),
                FieldPayloadRef::CategoryU32(
                    self.hydro_erosion
                        .snapshot()
                        .hydrology()
                        .strahler_order()
                        .raw_values(),
                ),
            ),
        ]
    }

    #[cfg(test)]
    pub(super) fn test_fixture() -> Self {
        use crate::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
        use crate::generators::natural::{
            legacy_planar_natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact,
            GeologicSpecArtifact, HydroErosionSpecArtifact, RulePackSetArtifact,
            TectonicSpecArtifact, WorldFormationSpecArtifact,
        };
        use crate::generators::spatial::PlanarSpaceArtifact;
        use crate::rules::{default_rule_pack_set, AuthorConstraints};
        use crate::world::natural::{
            ClimateSpec, GeologicSpec, HydroErosionSpec, TectonicSpec, WorldFormationSpec,
        };
        use crate::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed};

        let mut external = ExternalArtifacts::new();
        external
            .insert(PlanarSpaceArtifact::new(PlanarSpaceSpec {
                width: Meters::new(1_000_000.0).unwrap(),
                height: Meters::new(600_000.0).unwrap(),
                target_cell_count: 128,
                boundary: BoundaryCondition::Closed,
            }))
            .unwrap();
        external
            .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
            .unwrap();
        external
            .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
            .unwrap();
        external
            .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
            .unwrap();
        external
            .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
            .unwrap();
        external
            .insert(WorldFormationSpecArtifact::new(
                WorldFormationSpec::default(),
            ))
            .unwrap();
        external
            .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
            .unwrap();
        external
            .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
            .unwrap();
        let outcome = BuildEngine::new(legacy_planar_natural_foundation_graph().unwrap())
            .build(RootSeed::new(42), external, &mut MemoryStageCache::new())
            .unwrap();
        Self::build(
            outcome.artifacts.get::<SpatialArtifact>().unwrap(),
            outcome
                .artifacts
                .get::<ResolvedWorldFormationArtifact>()
                .unwrap(),
            outcome.artifacts.get::<TectonicArtifact>().unwrap(),
            outcome.artifacts.get::<MantleArtifact>().unwrap(),
            outcome.artifacts.get::<ReliefArtifact>().unwrap(),
            outcome.artifacts.get::<GeologicArtifact>().unwrap(),
            outcome
                .artifacts
                .get::<PreliminaryClimateArtifact>()
                .unwrap(),
            outcome.artifacts.get::<HydroErosionArtifact>().unwrap(),
            &outcome.report,
        )
        .unwrap()
    }
}

impl AppFieldDocument for LegacyPlanarNaturalFieldDocument {
    fn mesh(&self) -> &Arc<PreparedCellMesh> {
        &self.mesh
    }

    fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError> {
        FieldCatalog::from_payloads(&self.registry, self.payloads())
    }

    fn diagnostics(&self) -> &[OwnedViewDiagnostic] {
        &self.diagnostics
    }

    fn preferred_field(&self) -> Option<FieldId> {
        Some(surface_elevation_m_field_id())
    }

    fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode> {
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
        (field == &surface_elevation_m_field_id()).then_some(())?;
        self.registry.get(field)?;
        let sea_level = self.relief.snapshot().sea_level_m();
        let radius = self
            .hydro_erosion
            .snapshot()
            .surface()
            .surface_elevation_m()
            .values()
            .iter()
            .map(|value| (value - sea_level).abs())
            .fold(0.0_f32, f32::max);
        ValueRange::new(sea_level - radius, sea_level + radius)
            .ok()
            .map(DisplayRangeMode::Manual)
    }
}

#[derive(Debug, Error)]
pub(super) enum NaturalDisplayError {
    #[error(transparent)]
    Spatial(#[from] SpatialValidationError),
    #[error(transparent)]
    Formation(#[from] WorldFormationSpecError),
    #[error(transparent)]
    Tectonic(#[from] TectonicValidationError),
    #[error(transparent)]
    Mantle(#[from] MantleValidationError),
    #[error(transparent)]
    Relief(#[from] ReliefValidationError),
    #[error(transparent)]
    Geologic(#[from] GeologicValidationError),
    #[error(transparent)]
    Climate(#[from] ClimateValidationError),
    #[error(transparent)]
    HydroErosion(#[from] HydroErosionValidationError),
    #[error(transparent)]
    Registry(#[from] NaturalFieldRegistryError),
    #[error(transparent)]
    Display(#[from] DisplayPrepareError),
    #[error(transparent)]
    FieldView(#[from] FieldViewError),
    #[error("natural plate count cannot be represented by the display registry")]
    PlateCountOverflow,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::app::field_document::AppFieldDocument;
    use crate::engine::{
        BuildEngine, BuildReport, Diagnostic, DiagnosticContext, DiagnosticSeverity,
        ExternalArtifacts, MemoryStageCache,
    };
    use crate::generators::natural::{
        legacy_planar_natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact,
        GeologicArtifact, GeologicSpecArtifact, HydroErosionArtifact, HydroErosionSpecArtifact,
        MantleArtifact, PreliminaryClimateArtifact, ReliefArtifact, ResolvedWorldFormationArtifact,
        RulePackSetArtifact, TectonicArtifact, TectonicSpecArtifact, WorldFormationSpecArtifact,
    };
    use crate::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
    use crate::rules::{default_rule_pack_set, AuthorConstraints};
    use crate::view::{DisplayRangeMode, ViewDiagnosticSeverity};
    use crate::world::natural::{
        bedrock_kind_field_id, elevation_field_id, geothermal_potential_field_id,
        mantle_heat_flow_field_id, plate_id_field_id, plate_velocity_field_id,
        preliminary_annual_precipitation_mm_field_id, preliminary_mean_air_temperature_c_field_id,
        preliminary_prevailing_wind_m_s_field_id, surface_elevation_m_field_id,
        surface_water_kind_field_id, volcanic_offset_field_id, ClimateSpec, GeologicSpec,
        HydroErosionSpec, MonthlyScalarField, MonthlyVectorField, PreliminaryClimateSnapshot,
        TectonicSpec, WorldFormationPreset, WorldFormationSpec, CLIMATE_MONTH_COUNT,
        PRELIMINARY_CLIMATE_SCHEMA_V1,
    };
    use crate::world::spatial::Topology;
    use crate::world::{BoundaryCondition, CellId, Meters, PlanarSpaceSpec, RootSeed};

    use super::LegacyPlanarNaturalFieldDocument;

    fn build_document_with_diagnostic() -> LegacyPlanarNaturalFieldDocument {
        let mut external = ExternalArtifacts::new();
        external
            .insert(PlanarSpaceArtifact::new(PlanarSpaceSpec {
                width: Meters::new(1_000_000.0).unwrap(),
                height: Meters::new(600_000.0).unwrap(),
                target_cell_count: 128,
                boundary: BoundaryCondition::Closed,
            }))
            .unwrap();
        external
            .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
            .unwrap();
        external
            .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
            .unwrap();
        external
            .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
            .unwrap();
        external
            .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
            .unwrap();
        external
            .insert(WorldFormationSpecArtifact::new(
                WorldFormationSpec::default(),
            ))
            .unwrap();
        external
            .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
            .unwrap();
        external
            .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
            .unwrap();
        let mut outcome = BuildEngine::new(legacy_planar_natural_foundation_graph().unwrap())
            .build(RootSeed::new(42), external, &mut MemoryStageCache::new())
            .unwrap();
        outcome.report.push_diagnostic(
            Diagnostic::with_context(
                DiagnosticSeverity::Warning,
                "test.natural-warning",
                String::from("owned natural warning"),
                DiagnosticContext {
                    field_id: Some(elevation_field_id()),
                    cell_id: Some(CellId::from_raw(3)),
                    ..DiagnosticContext::default()
                },
            )
            .unwrap(),
        );
        let spatial = outcome.artifacts.get::<SpatialArtifact>().unwrap();
        let formation = outcome
            .artifacts
            .get::<ResolvedWorldFormationArtifact>()
            .unwrap();
        let tectonic = outcome.artifacts.get::<TectonicArtifact>().unwrap();
        let mantle = outcome.artifacts.get::<MantleArtifact>().unwrap();
        let relief = outcome.artifacts.get::<ReliefArtifact>().unwrap();
        let geology = outcome.artifacts.get::<GeologicArtifact>().unwrap();
        let climate = outcome
            .artifacts
            .get::<PreliminaryClimateArtifact>()
            .unwrap();
        let hydro_erosion = outcome.artifacts.get::<HydroErosionArtifact>().unwrap();
        LegacyPlanarNaturalFieldDocument::build(
            spatial,
            formation,
            tectonic,
            mantle,
            relief,
            geology,
            climate,
            hydro_erosion,
            &outcome.report,
        )
        .unwrap()
    }

    #[test]
    fn document_borrows_formal_fields_and_derives_cell_velocity() {
        let document = build_document_with_diagnostic();
        assert_eq!(
            document.formation.formation().requested(),
            WorldFormationPreset::Continents
        );
        let catalog = document.catalog().unwrap();
        assert!(catalog.entries().iter().all(|entry| entry.view().is_some()));
        let plate_values = catalog
            .get(&plate_id_field_id())
            .unwrap()
            .view()
            .unwrap()
            .category_values()
            .unwrap();
        assert_eq!(
            plate_values.as_ptr(),
            document
                .tectonic
                .snapshot()
                .cell_plates()
                .raw_values()
                .as_ptr()
        );
        let elevation_values = catalog
            .get(&elevation_field_id())
            .unwrap()
            .view()
            .unwrap()
            .scalar_values()
            .unwrap();
        assert_eq!(
            elevation_values.as_ptr(),
            document.relief.snapshot().elevation_m().values().as_ptr()
        );
        assert_eq!(
            catalog
                .get(&mantle_heat_flow_field_id())
                .unwrap()
                .view()
                .unwrap()
                .scalar_values()
                .unwrap()
                .as_ptr(),
            document.mantle.snapshot().heat_flow_mw_m2().as_ptr()
        );
        assert_eq!(
            catalog
                .get(&volcanic_offset_field_id())
                .unwrap()
                .view()
                .unwrap()
                .scalar_values()
                .unwrap()
                .as_ptr(),
            document
                .relief
                .snapshot()
                .volcanic_offset_m()
                .values()
                .as_ptr()
        );
        assert_eq!(
            catalog
                .get(&bedrock_kind_field_id())
                .unwrap()
                .view()
                .unwrap()
                .category_values()
                .unwrap()
                .as_ptr(),
            document
                .geology
                .snapshot()
                .bedrock_kinds()
                .raw_values()
                .as_ptr()
        );
        assert_eq!(
            catalog
                .get(&geothermal_potential_field_id())
                .unwrap()
                .view()
                .unwrap()
                .scalar_values()
                .unwrap()
                .as_ptr(),
            document.geology.snapshot().geothermal_potential().as_ptr()
        );
        assert_eq!(
            catalog
                .get(&preliminary_mean_air_temperature_c_field_id())
                .unwrap()
                .view()
                .unwrap()
                .scalar_values()
                .unwrap()
                .as_ptr(),
            document
                .climate
                .snapshot()
                .mean_annual_air_temperature_c()
                .as_ptr()
        );
        assert_eq!(
            catalog
                .get(&preliminary_annual_precipitation_mm_field_id())
                .unwrap()
                .view()
                .unwrap()
                .scalar_values()
                .unwrap()
                .as_ptr(),
            document
                .climate
                .snapshot()
                .annual_precipitation_mm()
                .as_ptr()
        );
        assert_eq!(
            catalog
                .get(&preliminary_prevailing_wind_m_s_field_id())
                .unwrap()
                .view()
                .unwrap()
                .vector_values()
                .unwrap()
                .as_ptr(),
            document.climate.snapshot().prevailing_wind_m_s().as_ptr()
        );
        assert_eq!(
            catalog
                .get(&surface_elevation_m_field_id())
                .unwrap()
                .view()
                .unwrap()
                .scalar_values()
                .unwrap()
                .as_ptr(),
            document
                .hydro_erosion
                .snapshot()
                .surface()
                .surface_elevation_m()
                .values()
                .as_ptr()
        );
        assert_eq!(
            catalog
                .get(&surface_water_kind_field_id())
                .unwrap()
                .view()
                .unwrap()
                .category_values()
                .unwrap()
                .as_ptr(),
            document
                .hydro_erosion
                .snapshot()
                .hydrology()
                .surface_water()
                .raw_values()
                .as_ptr()
        );

        let velocities = catalog
            .get(&plate_velocity_field_id())
            .unwrap()
            .view()
            .unwrap()
            .vector_values()
            .unwrap();
        for (cell, velocity) in velocities.iter().enumerate() {
            let plate = document
                .tectonic
                .snapshot()
                .cell_plates()
                .get(cell)
                .unwrap();
            let expected = document.tectonic.snapshot().plates()[plate.raw() as usize]
                .velocity
                .components_mm_per_year();
            assert_eq!(
                *velocity,
                [f32::from(expected[0]) / 10.0, f32::from(expected[1]) / 10.0]
            );
        }
    }

    #[test]
    fn complete_spatial_snapshot_builds_a_complete_mesh() {
        let document = build_document_with_diagnostic();
        assert_eq!(
            document.mesh().cell_count(),
            document.spatial.snapshot().cell_count()
        );
        let mut seen = vec![false; document.mesh().cell_count()];
        for vertex in document.mesh().vertices() {
            seen[vertex.cell as usize] = true;
        }
        assert!(seen.into_iter().all(|present| present));
    }

    #[test]
    fn document_rejects_a_self_valid_but_spatially_misaligned_climate() {
        let document = build_document_with_diagnostic();
        let monthly_scalar =
            || MonthlyScalarField::from_values(vec![[0.0; CLIMATE_MONTH_COUNT]]).unwrap();
        let monthly_vector =
            MonthlyVectorField::from_values(vec![[[0.0; 2]; CLIMATE_MONTH_COUNT]]).unwrap();
        let climate = Arc::new(PreliminaryClimateArtifact::new(
            PreliminaryClimateSnapshot::new(
                PRELIMINARY_CLIMATE_SCHEMA_V1,
                1,
                vec![0.0],
                vec![0.0],
                monthly_scalar(),
                monthly_scalar(),
                monthly_vector,
                vec![0.0],
                vec![0.0],
                vec![0.0],
                vec![[0.0; 2]],
            )
            .unwrap(),
        ));

        let result = LegacyPlanarNaturalFieldDocument::build(
            document.spatial.clone(),
            document.formation.clone(),
            document.tectonic.clone(),
            document.mantle.clone(),
            document.relief.clone(),
            document.geology.clone(),
            climate,
            document.hydro_erosion.clone(),
            &BuildReport::new(),
        );
        assert!(matches!(
            result,
            Err(super::NaturalDisplayError::Climate(_))
        ));
    }

    #[test]
    fn diagnostics_are_owned_and_default_display_is_symmetric_current_surface() {
        let document = build_document_with_diagnostic();
        assert_eq!(document.diagnostics().len(), 1);
        assert_eq!(
            document.diagnostics()[0].severity,
            ViewDiagnosticSeverity::Warning
        );
        assert_eq!(document.diagnostics()[0].message, "owned natural warning");
        assert_eq!(
            document.preferred_field(),
            Some(surface_elevation_m_field_id())
        );
        let DisplayRangeMode::Manual(range) = document
            .preferred_range(&surface_elevation_m_field_id())
            .expect("current surface has a preferred sea-level range")
        else {
            panic!("current surface must use an explicit symmetric range");
        };
        let sea_level = document.relief.snapshot().sea_level_m();
        assert!(((range.min() + range.max()) * 0.5 - sea_level).abs() < 0.001);
        let expected_radius = document
            .hydro_erosion
            .snapshot()
            .surface()
            .surface_elevation_m()
            .values()
            .iter()
            .map(|value| (value - sea_level).abs())
            .fold(0.0_f32, f32::max);
        assert!((range.max() - sea_level - expected_radius).abs() < 0.001);
    }
}
