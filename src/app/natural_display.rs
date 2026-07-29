use std::sync::Arc;

use thiserror::Error;

use super::field_document::AppFieldDocument;
use crate::engine::{BuildReport, DiagnosticSeverity};
use crate::generators::natural::{ReliefArtifact, TectonicArtifact};
use crate::generators::spatial::SpatialArtifact;
use crate::view::{
    DisplayPrepareError, DisplayRangeMode, FieldCatalog, FieldPayloadRef, FieldViewError,
    MeshCompleteness, OwnedViewDiagnostic, PreparedCellMesh, ViewDiagnosticSeverity,
};
use crate::world::fields::{FieldId, FieldRegistry, ValueRange};
use crate::world::natural::{
    boundary_kind_field_id, boundary_strength_field_id, crust_base_elevation_field_id,
    crust_kind_field_id, crust_thickness_field_id, elevation_field_id, land_ocean_field_id,
    natural_field_registry, plate_id_field_id, plate_velocity_field_id, regional_offset_field_id,
    tectonic_offset_field_id, NaturalFieldDisplayCache, NaturalFieldRegistryError,
    ReliefValidationError, TectonicValidationError,
};
use crate::world::spatial::SpatialValidationError;

/// Immutable formal natural-world document used by the application display boundary.
pub(super) struct NaturalFieldDocument {
    pub(super) spatial: Arc<SpatialArtifact>,
    pub(super) tectonic: Arc<TectonicArtifact>,
    pub(super) relief: Arc<ReliefArtifact>,
    registry: FieldRegistry,
    mesh: Arc<PreparedCellMesh>,
    diagnostics: Vec<OwnedViewDiagnostic>,
    display_cache: NaturalFieldDisplayCache,
}

impl NaturalFieldDocument {
    pub(super) fn build(
        spatial: Arc<SpatialArtifact>,
        tectonic: Arc<TectonicArtifact>,
        relief: Arc<ReliefArtifact>,
        report: &BuildReport,
    ) -> Result<Self, NaturalDisplayError> {
        spatial.snapshot().validate()?;
        tectonic.snapshot().validate_against(spatial.snapshot())?;
        relief.snapshot().validate_against(spatial.snapshot())?;
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
            tectonic,
            relief,
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
        ]
    }

    #[cfg(test)]
    pub(super) fn test_fixture() -> Self {
        use crate::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
        use crate::generators::natural::{natural_foundation_graph, TectonicSpecArtifact};
        use crate::generators::spatial::PlanarSpaceArtifact;
        use crate::world::natural::TectonicSpec;
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
        let outcome = BuildEngine::new(natural_foundation_graph().unwrap())
            .build(RootSeed::new(42), external, &mut MemoryStageCache::new())
            .unwrap();
        Self::build(
            outcome.artifacts.get::<SpatialArtifact>().unwrap(),
            outcome.artifacts.get::<TectonicArtifact>().unwrap(),
            outcome.artifacts.get::<ReliefArtifact>().unwrap(),
            &outcome.report,
        )
        .unwrap()
    }
}

impl AppFieldDocument for NaturalFieldDocument {
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
        Some(elevation_field_id())
    }

    fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode> {
        if field != &elevation_field_id() {
            return None;
        }
        let schema = self.registry.get(field)?;
        let schema_range = schema.valid_range?;
        let sea_level = self.relief.snapshot().sea_level_m();
        let radius = (schema_range.min() - sea_level)
            .abs()
            .max((schema_range.max() - sea_level).abs());
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
    Tectonic(#[from] TectonicValidationError),
    #[error(transparent)]
    Relief(#[from] ReliefValidationError),
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
    use crate::app::field_document::AppFieldDocument;
    use crate::engine::{
        BuildEngine, Diagnostic, DiagnosticContext, DiagnosticSeverity, ExternalArtifacts,
        MemoryStageCache,
    };
    use crate::generators::natural::{
        natural_foundation_graph, ReliefArtifact, TectonicArtifact, TectonicSpecArtifact,
    };
    use crate::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
    use crate::view::{DisplayRangeMode, ViewDiagnosticSeverity};
    use crate::world::natural::{
        elevation_field_id, plate_id_field_id, plate_velocity_field_id, TectonicSpec,
    };
    use crate::world::spatial::Topology;
    use crate::world::{BoundaryCondition, CellId, Meters, PlanarSpaceSpec, RootSeed};

    use super::NaturalFieldDocument;

    fn build_document_with_diagnostic() -> NaturalFieldDocument {
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
        let mut outcome = BuildEngine::new(natural_foundation_graph().unwrap())
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
        let tectonic = outcome.artifacts.get::<TectonicArtifact>().unwrap();
        let relief = outcome.artifacts.get::<ReliefArtifact>().unwrap();
        NaturalFieldDocument::build(spatial, tectonic, relief, &outcome.report).unwrap()
    }

    #[test]
    fn document_borrows_formal_fields_and_derives_cell_velocity() {
        let document = build_document_with_diagnostic();
        let catalog = document.catalog().unwrap();
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
    fn diagnostics_are_owned_and_default_display_is_symmetric_elevation() {
        let document = build_document_with_diagnostic();
        assert_eq!(document.diagnostics().len(), 1);
        assert_eq!(
            document.diagnostics()[0].severity,
            ViewDiagnosticSeverity::Warning
        );
        assert_eq!(document.diagnostics()[0].message, "owned natural warning");
        assert_eq!(document.preferred_field(), Some(elevation_field_id()));
        let DisplayRangeMode::Manual(range) = document
            .preferred_range(&elevation_field_id())
            .expect("elevation has a preferred sea-level range")
        else {
            panic!("natural elevation must use an explicit symmetric range");
        };
        let sea_level = document.relief.snapshot().sea_level_m();
        assert!(((range.min() + range.max()) * 0.5 - sea_level).abs() < 0.001);
    }
}
