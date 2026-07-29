use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use thiserror::Error;

use crate::delaunay::voronoi::IndexedVoronoiDiagram;
use crate::view::{
    CellGeometrySource, DisplayPrepareError, MeshCompleteness, OwnedViewDiagnostic,
    PreparedCellMesh, ViewDiagnosticSeverity,
};
use crate::world::fields::{
    DomainSizes, ExtensionFieldSet, FieldData, FieldDataError, FieldDisplayMetadata, FieldDomain,
    FieldId, FieldPaletteHint, FieldRegistry, FieldRegistryBuilder, FieldSchema, FieldSchemaError,
    FieldUnit, FieldValueType, MissingValuePolicy, ValueRange,
};
use crate::world::{CellId, Meters, WorldPoint, WorldRect};

const MAX_GEOMETRY_WARNINGS: usize = 64;

/// Owned renderer-neutral document adapted from the current legacy map generator.
pub(super) struct LegacyTerrainDisplay {
    pub(super) registry: FieldRegistry,
    pub(super) fields: ExtensionFieldSet,
    pub(super) mesh: Arc<PreparedCellMesh>,
    pub(super) diagnostics: Vec<OwnedViewDiagnostic>,
}

/// One-way adapter from current height, plate, and Voronoi outputs.
pub(super) struct LegacyTerrainDisplayAdapter;

impl LegacyTerrainDisplayAdapter {
    /// Validates and adapts one complete legacy generation result.
    pub(super) fn build(
        bounds: WorldRect,
        voronoi: &IndexedVoronoiDiagram,
        heights: &[u8],
        plate_ids: &[u16],
    ) -> Result<LegacyTerrainDisplay, LegacyDisplayError> {
        let cell_count = voronoi.cells.len();
        if heights.len() != cell_count || plate_ids.len() != cell_count {
            return Err(LegacyDisplayError::OutputLengthMismatch {
                cells: cell_count,
                heights: heights.len(),
                plate_ids: plate_ids.len(),
            });
        }

        let (geometry, diagnostics) =
            LegacyCellGeometry::from_voronoi(bounds, voronoi, cell_count)?;
        let mesh = Arc::new(PreparedCellMesh::build(
            &geometry,
            MeshCompleteness::AllowMissing,
        )?);
        let registry = build_registry(plate_ids)?;
        let sizes = DomainSizes::new(cell_count, 0);
        let mut fields = ExtensionFieldSet::new();
        fields.insert(
            &registry,
            legacy_elevation_id(),
            FieldData::ScalarF32(heights.iter().map(|height| f32::from(*height)).collect()),
            &sizes,
        )?;
        fields.insert(
            &registry,
            legacy_plate_id(),
            FieldData::CategoryU32(plate_ids.iter().map(|plate| u32::from(*plate)).collect()),
            &sizes,
        )?;

        Ok(LegacyTerrainDisplay {
            registry,
            fields,
            mesh,
            diagnostics,
        })
    }
}

/// Errors at the one-way legacy display boundary.
#[derive(Debug, Error)]
pub(super) enum LegacyDisplayError {
    /// Generator arrays did not match the stable Voronoi cell cardinality.
    #[error(
        "legacy output length mismatch: cells={cells}, heights={heights}, plate_ids={plate_ids}"
    )]
    OutputLengthMismatch {
        /// Voronoi cell records.
        cells: usize,
        /// Generated heights.
        heights: usize,
        /// Generated plate identifiers.
        plate_ids: usize,
    },
    /// Two legacy cell records claimed the same stable site.
    #[error("duplicate legacy Voronoi site {site}")]
    DuplicateSite {
        /// The duplicated raw site index.
        site: u32,
    },
    /// A legacy cell record claimed a site outside the output arrays.
    #[error("legacy Voronoi site {site} is outside cell count {cell_count}")]
    SiteOutOfRange {
        /// The invalid raw site index.
        site: u32,
        /// Valid cell cardinality.
        cell_count: usize,
    },
    /// Constant legacy field schema construction failed.
    #[error(transparent)]
    Schema(#[from] FieldSchemaError),
    /// Adapted payload validation failed.
    #[error(transparent)]
    FieldData(#[from] FieldDataError),
    /// Prepared display geometry failed validation or a budget.
    #[error(transparent)]
    Display(#[from] DisplayPrepareError),
}

pub(super) fn legacy_elevation_id() -> FieldId {
    FieldId::new("sekai.legacy", "elevation", 1)
        .expect("the built-in legacy elevation field identifier is valid")
}

pub(super) fn legacy_plate_id() -> FieldId {
    FieldId::new("sekai.legacy", "plate_id", 1)
        .expect("the built-in legacy plate field identifier is valid")
}

fn build_registry(plate_ids: &[u16]) -> Result<FieldRegistry, FieldSchemaError> {
    let elevation = FieldSchema {
        id: legacy_elevation_id(),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::ScalarF32,
        unit: FieldUnit::Unitless,
        valid_range: Some(ValueRange::new(0.0, 255.0)?),
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new(
            "field.legacy.elevation",
            FieldPaletteHint::Sequential,
            0,
        )?,
    };
    let category_labels = plate_ids
        .iter()
        .copied()
        .map(u32::from)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|plate| (plate, format!("plate.{plate}")))
        .collect();
    let plates = FieldSchema {
        id: legacy_plate_id(),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::CategoryU32,
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels,
        display: FieldDisplayMetadata::new(
            "field.legacy.plate_id",
            FieldPaletteHint::Categorical,
            0,
        )?,
    };
    let mut builder = FieldRegistryBuilder::new();
    builder.register(elevation)?;
    builder.register(plates)?;
    builder.build()
}

struct LegacyCellGeometry {
    bounds: WorldRect,
    polygons: Vec<Option<Vec<WorldPoint>>>,
}

impl LegacyCellGeometry {
    fn from_voronoi(
        bounds: WorldRect,
        voronoi: &IndexedVoronoiDiagram,
        cell_count: usize,
    ) -> Result<(Self, Vec<OwnedViewDiagnostic>), LegacyDisplayError> {
        let mut polygons = vec![None; cell_count];
        let mut seen = vec![false; cell_count];
        let mut warnings = GeometryWarnings::default();
        for cell in &voronoi.cells {
            let site =
                usize::try_from(cell.site_idx).map_err(|_| LegacyDisplayError::SiteOutOfRange {
                    site: cell.site_idx,
                    cell_count,
                })?;
            if site >= cell_count {
                return Err(LegacyDisplayError::SiteOutOfRange {
                    site: cell.site_idx,
                    cell_count,
                });
            }
            if std::mem::replace(&mut seen[site], true) {
                return Err(LegacyDisplayError::DuplicateSite {
                    site: cell.site_idx,
                });
            }
            let stable_cell = CellId::from_raw(cell.site_idx);
            let Some(polygon) = convert_polygon(bounds, voronoi, &cell.vertex_indices) else {
                warnings.omit(stable_cell);
                continue;
            };
            polygons[site] = Some(polygon);
        }
        for (site, was_seen) in seen.into_iter().enumerate() {
            if !was_seen {
                let raw = u32::try_from(site).map_err(|_| LegacyDisplayError::SiteOutOfRange {
                    site: u32::MAX,
                    cell_count,
                })?;
                warnings.omit(CellId::from_raw(raw));
            }
        }
        Ok((Self { bounds, polygons }, warnings.finish()))
    }
}

impl CellGeometrySource for LegacyCellGeometry {
    fn bounds(&self) -> WorldRect {
        self.bounds
    }

    fn cell_count(&self) -> usize {
        self.polygons.len()
    }

    fn polygon(&self, cell: CellId) -> Option<&[WorldPoint]> {
        self.polygons
            .get(cell.raw() as usize)
            .and_then(Option::as_deref)
    }
}

fn convert_polygon(
    bounds: WorldRect,
    voronoi: &IndexedVoronoiDiagram,
    indices: &[u32],
) -> Option<Vec<WorldPoint>> {
    if indices.len() < 3 {
        return None;
    }
    let mut polygon = Vec::with_capacity(indices.len());
    for index in indices {
        let position = voronoi.vertices.get(*index as usize)?;
        let x = Meters::new(f64::from(position.x)).ok()?;
        let y = Meters::new(f64::from(position.y)).ok()?;
        let point = WorldPoint::new(x, y);
        if !bounds.contains(point) {
            return None;
        }
        polygon.push(point);
    }

    let area = signed_area(&polygon);
    if !area.is_finite() || area.abs() <= f64::EPSILON {
        return None;
    }
    if area < 0.0 {
        polygon.reverse();
    }
    is_renderable_convex(bounds, &polygon).then_some(polygon)
}

fn signed_area(polygon: &[WorldPoint]) -> f64 {
    polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
        .map(|(a, b)| a.x().get() * b.y().get() - b.x().get() * a.y().get())
        .sum::<f64>()
        * 0.5
}

fn is_renderable_convex(bounds: WorldRect, polygon: &[WorldPoint]) -> bool {
    let width = bounds.width().get();
    let height = bounds.height().get();
    let normalized: Vec<_> = polygon
        .iter()
        .map(|point| {
            [
                ((point.x().get() - bounds.min().x().get()) / width) as f32,
                ((point.y().get() - bounds.min().y().get()) / height) as f32,
            ]
        })
        .collect();
    for index in 0..normalized.len() {
        let a = normalized[index];
        let b = normalized[(index + 1) % normalized.len()];
        let c = normalized[(index + 2) % normalized.len()];
        if display_cross(a, b, c) <= 0.0 {
            return false;
        }
    }
    let fan_origin = normalized[0];
    // Dense legacy cells can have valid normalized areas below any fixed global epsilon.
    normalized[1..]
        .windows(2)
        .all(|triangle| display_cross(fan_origin, triangle[0], triangle[1]) > 0.0)
}

fn display_cross(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

#[derive(Default)]
struct GeometryWarnings {
    omitted: usize,
    diagnostics: Vec<OwnedViewDiagnostic>,
}

impl GeometryWarnings {
    fn omit(&mut self, cell: CellId) {
        self.omitted += 1;
        if self.diagnostics.len() < MAX_GEOMETRY_WARNINGS {
            self.diagnostics.push(OwnedViewDiagnostic {
                severity: ViewDiagnosticSeverity::Warning,
                code: "display.legacy.geometry".into(),
                field_id: None,
                cell_id: Some(cell),
                message: "旧 Voronoi 单元格几何无效，已从填色网格省略".into(),
            });
        }
    }

    fn finish(mut self) -> Vec<OwnedViewDiagnostic> {
        if self.omitted > MAX_GEOMETRY_WARNINGS {
            self.diagnostics.push(OwnedViewDiagnostic {
                severity: ViewDiagnosticSeverity::Warning,
                code: "display.legacy.geometry.summary".into(),
                field_id: None,
                cell_id: None,
                message: format!(
                    "另有 {} 个无效旧 Voronoi 单元格未逐项列出",
                    self.omitted - MAX_GEOMETRY_WARNINGS
                ),
            });
        }
        self.diagnostics
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{prepare_control_action, prepare_new_legacy_display};
    use super::{
        legacy_elevation_id, legacy_plate_id, LegacyDisplayError, LegacyTerrainDisplayAdapter,
    };
    use crate::delaunay::voronoi::{IndexedVoronoiDiagram, VoronoiCell};
    use crate::ui::field::FieldControlAction;
    use crate::view::{
        DiagnosticScope, DisplayRangeMode, DisplayRevisionClock, FieldCatalog, FieldDisplayState,
        OwnedViewDiagnostic, ViewDiagnosticSeverity,
    };
    use crate::world::fields::ValueRange;
    use crate::world::{CellId, Meters, WorldPoint, WorldRect};

    fn point(x: f64, y: f64) -> WorldPoint {
        WorldPoint::new(Meters::new(x).unwrap(), Meters::new(y).unwrap())
    }

    fn four_cell_legacy_geometry() -> (WorldRect, IndexedVoronoiDiagram) {
        let bounds = WorldRect::new(point(0.0, 0.0), point(2.0, 2.0)).unwrap();
        let vertices = vec![
            egui::pos2(0.0, 0.0),
            egui::pos2(1.0, 0.0),
            egui::pos2(2.0, 0.0),
            egui::pos2(0.0, 1.0),
            egui::pos2(1.0, 1.0),
            egui::pos2(2.0, 1.0),
            egui::pos2(0.0, 2.0),
            egui::pos2(1.0, 2.0),
            egui::pos2(2.0, 2.0),
        ];
        let cells = vec![
            VoronoiCell {
                site_idx: 0,
                vertex_indices: vec![0, 1, 4, 3],
            },
            VoronoiCell {
                site_idx: 1,
                vertex_indices: vec![1, 2, 5, 4],
            },
            VoronoiCell {
                site_idx: 2,
                vertex_indices: vec![3, 4, 7, 6],
            },
            VoronoiCell {
                site_idx: 3,
                vertex_indices: vec![4, 5, 8, 7],
            },
        ];
        (
            bounds,
            IndexedVoronoiDiagram {
                vertices,
                indices: Vec::new(),
                cells,
            },
        )
    }

    #[test]
    fn adapter_exposes_height_and_plate_fields_with_valid_schemas() {
        let (bounds, voronoi) = four_cell_legacy_geometry();
        let heights = vec![0, 64, 128, 255];
        let plates = vec![2, 2, 9, 9];
        let display =
            LegacyTerrainDisplayAdapter::build(bounds, &voronoi, &heights, &plates).unwrap();
        let catalog =
            FieldCatalog::from_extension_fields(&display.registry, &display.fields).unwrap();

        let elevation = catalog.get(&legacy_elevation_id()).unwrap().view().unwrap();
        assert_eq!(
            elevation.scalar_values(),
            Some(&[0.0, 64.0, 128.0, 255.0][..])
        );
        assert_eq!(elevation.schema().valid_range.unwrap().min(), 0.0);
        assert_eq!(elevation.schema().valid_range.unwrap().max(), 255.0);

        let plate = catalog.get(&legacy_plate_id()).unwrap().view().unwrap();
        assert_eq!(plate.category_values(), Some(&[2, 2, 9, 9][..]));
        assert_eq!(
            plate
                .schema()
                .category_labels
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![2, 9]
        );
        assert_eq!(display.mesh.cell_count(), 4);
        assert!(display.diagnostics.is_empty());
    }

    #[test]
    fn adapter_rejects_mismatched_generation_outputs_atomically() {
        let (bounds, voronoi) = four_cell_legacy_geometry();
        assert!(matches!(
            LegacyTerrainDisplayAdapter::build(bounds, &voronoi, &[1, 2], &[0]),
            Err(LegacyDisplayError::OutputLengthMismatch { .. })
        ));
    }

    #[test]
    fn malformed_cells_are_omitted_with_owned_warnings() {
        let (bounds, mut voronoi) = four_cell_legacy_geometry();
        voronoi.cells[1].vertex_indices = vec![1, 2];
        let display =
            LegacyTerrainDisplayAdapter::build(bounds, &voronoi, &[0; 4], &[1; 4]).unwrap();

        assert_eq!(display.mesh.cell_count(), 4);
        assert_eq!(display.diagnostics.len(), 1);
        assert_eq!(
            display.diagnostics[0].cell_id,
            Some(crate::world::CellId::from_raw(1))
        );
        assert_eq!(display.diagnostics[0].code, "display.legacy.geometry");
    }

    #[test]
    fn repeated_legacy_polygon_vertices_are_omitted_before_mesh_preparation() {
        let (bounds, mut voronoi) = four_cell_legacy_geometry();
        voronoi.cells[1].vertex_indices = vec![1, 2, 2, 5, 4];
        let display =
            LegacyTerrainDisplayAdapter::build(bounds, &voronoi, &[0; 4], &[1; 4]).unwrap();

        assert_eq!(display.mesh.cell_count(), 4);
        assert_eq!(
            display.mesh.pick_local([1.5, 0.5]),
            None,
            "the malformed cell must not produce degenerate GPU triangles"
        );
        assert_eq!(display.diagnostics.len(), 1);
        assert_eq!(display.diagnostics[0].cell_id, Some(CellId::from_raw(1)));
    }

    #[test]
    fn narrow_positive_legacy_triangles_remain_fillable() {
        let bounds = WorldRect::new(point(0.0, 0.0), point(1.0, 1.0)).unwrap();
        let voronoi = IndexedVoronoiDiagram {
            vertices: vec![
                egui::pos2(0.0, 0.0),
                egui::pos2(0.01, 0.0),
                egui::pos2(0.010005, 0.000005),
                egui::pos2(0.01, 0.01),
                egui::pos2(0.0, 0.01),
            ],
            indices: Vec::new(),
            cells: vec![VoronoiCell {
                site_idx: 0,
                vertex_indices: vec![0, 1, 2, 3, 4],
            }],
        };

        let display = LegacyTerrainDisplayAdapter::build(bounds, &voronoi, &[0], &[1]).unwrap();

        assert!(
            display.diagnostics.is_empty(),
            "strictly positive display triangles are valid even when their area is small"
        );
        assert_eq!(
            display.mesh.pick_local([0.005, 0.005]),
            Some(CellId::from_raw(0))
        );
    }

    #[test]
    fn duplicate_site_indices_are_identity_errors_not_silent_geometry_loss() {
        let (bounds, mut voronoi) = four_cell_legacy_geometry();
        voronoi.cells[1].site_idx = 0;
        assert!(matches!(
            LegacyTerrainDisplayAdapter::build(bounds, &voronoi, &[0; 4], &[1; 4]),
            Err(LegacyDisplayError::DuplicateSite { site: 0 })
        ));
    }

    #[test]
    fn control_actions_invalidate_only_their_owned_display_inputs() {
        let (bounds, voronoi) = four_cell_legacy_geometry();
        let mut display =
            LegacyTerrainDisplayAdapter::build(bounds, &voronoi, &[0, 64, 128, 255], &[2, 2, 9, 9])
                .unwrap();
        display.diagnostics.push(OwnedViewDiagnostic {
            severity: ViewDiagnosticSeverity::Warning,
            code: "test.plate".into(),
            field_id: Some(legacy_plate_id()),
            cell_id: Some(CellId::from_raw(1)),
            message: "plate warning".into(),
        });
        let mut clock = DisplayRevisionClock::default();
        let (mut state, initial) =
            prepare_new_legacy_display(&display, &FieldDisplayState::default(), &mut clock)
                .unwrap();
        let initial_revisions = initial.revisions();

        let ranged = prepare_control_action(
            &display,
            &initial,
            &mut state,
            &mut clock,
            FieldControlAction::SetRangeMode(DisplayRangeMode::Manual(
                ValueRange::new(32.0, 224.0).unwrap(),
            )),
        )
        .unwrap();
        assert_eq!(ranged.revisions(), initial_revisions);
        assert!(Arc::ptr_eq(ranged.field_arc(), initial.field_arc()));
        assert_eq!(ranged.display_range().unwrap().bounds(), (32.0, 224.0));

        let toggled = prepare_control_action(
            &display,
            &ranged,
            &mut state,
            &mut clock,
            FieldControlAction::SetDiagnosticsEnabled(false),
        )
        .unwrap();
        assert_eq!(toggled.revisions(), initial_revisions);
        assert!(!toggled.diagnostics_enabled());

        let all_diagnostics = prepare_control_action(
            &display,
            &toggled,
            &mut state,
            &mut clock,
            FieldControlAction::SetDiagnosticScope(DiagnosticScope::AllFields),
        )
        .unwrap();
        assert_eq!(all_diagnostics.revisions().mesh, initial_revisions.mesh);
        assert_eq!(all_diagnostics.revisions().field, initial_revisions.field);
        assert_ne!(
            all_diagnostics.revisions().diagnostics,
            initial_revisions.diagnostics
        );
        assert_eq!(
            all_diagnostics.revisions().palette,
            initial_revisions.palette
        );

        let plates = prepare_control_action(
            &display,
            &all_diagnostics,
            &mut state,
            &mut clock,
            FieldControlAction::SelectField(legacy_plate_id()),
        )
        .unwrap();
        assert_eq!(plates.revisions().mesh, initial_revisions.mesh);
        assert_ne!(plates.revisions().field, initial_revisions.field);
        assert_eq!(
            plates.revisions().diagnostics,
            all_diagnostics.revisions().diagnostics
        );
        assert_ne!(plates.revisions().palette, initial_revisions.palette);
        assert_eq!(state.selected_field(), Some(&legacy_plate_id()));
    }
}
