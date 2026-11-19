//! Field document for the independently generated Atlas current state.

use crate::ui::field::localization as text;
use std::sync::Arc;

use super::field_document::{owned_view_diagnostics, FieldDocument};
use super::spherical_formation_display::FormationDisplayCache;
use crate::engine::BuildOutcome;
use crate::generators::natural::AtlasTerrainArtifact;
use crate::view::{
    DisplayRangeMode, FieldCatalog, FieldPayloadRef, FieldViewError, OwnedViewDiagnostic,
    SphericalPresentationSource,
};
use crate::world::fields::{FieldId, FieldRegistry};
use crate::world::natural::{
    atlas_field_registry, circulation_surface_albedo_field_id, land_ocean_field_id,
    surface_elevation_m_field_id, surface_water_kind_field_id, AtlasTerrainSnapshot,
    NaturalQualityProfile, SurfaceWaterField,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};

/// Immutable current terrain, initialized water and current climate from one verified build.
pub struct AtlasFieldDocument {
    artifact: Arc<AtlasTerrainArtifact>,
    source: SphericalPresentationSource,
    registry: FieldRegistry,
    cache: FormationDisplayCache,
    diagnostics: Vec<OwnedViewDiagnostic>,
    pub(super) water: SurfaceWaterField,
    pub(super) display_radius_m: f32,
    source_cell_spacing_m: f64,
    river_flow_range: Option<(f64, f64)>,
}

impl AtlasFieldDocument {
    pub(super) fn from_build_outcome(outcome: &BuildOutcome) -> Result<Self, String> {
        let provenance = outcome.verified_provenance().map_err(|e| e.to_string())?;
        let artifact = outcome
            .artifacts
            .get::<AtlasTerrainArtifact>()
            .map_err(|e| e.to_string())?;
        let snapshot = artifact.snapshot();
        let surface = snapshot.surface();
        let params = snapshot.fine_terrain().coarse_terrain().params();
        if u64::from(params.seed) != provenance.root_seed().raw() {
            return Err("Atlas source seed differs from the engine root seed".into());
        }
        // u8 sectors are the existing display representation, not a topology assumption.
        for cell in surface.cells() {
            super::atlas_detail::checked_sector_count(cell.boundary_vertices.len())?;
        }
        let surface_ref = SurfaceRef::try_for_spherical(surface).map_err(|e| e.to_string())?;
        let source = SphericalPresentationSource::new(
            provenance.root_seed(),
            surface_ref,
            *provenance.result_hash(),
            1,
        );
        let registry = atlas_field_registry().map_err(|e| e.to_string())?;
        let cache = FormationDisplayCache::build(surface, snapshot.climate().fields())
            .map_err(|e| e.to_string())?;
        let source_cell_spacing_m =
            (surface.total_cell_area().get() / surface.cells().len() as f64).sqrt();
        let water = snapshot.source_water_field();
        let display_radius_m = super::natural_field_payloads::elevation_display_radius_m(
            0.0,
            snapshot.fields().bed_elevation_m(),
        )
        .ok_or("Atlas display has no finite elevation range")?;
        let river_flow_range = snapshot.rivers().first().map(|first| {
            snapshot.rivers().iter().fold(
                (first.mean_discharge_m3_s, first.mean_discharge_m3_s),
                |(min, max), river| {
                    (
                        min.min(river.mean_discharge_m3_s),
                        max.max(river.mean_discharge_m3_s),
                    )
                },
            )
        });
        let document = Self {
            artifact,
            source,
            registry,
            cache,
            water,
            display_radius_m,
            source_cell_spacing_m,
            river_flow_range,
            diagnostics: owned_view_diagnostics(&outcome.report),
        };
        document.catalog().map_err(|e| e.to_string())?;
        Ok(document)
    }

    /// Returns the same final snapshot used for terrain, water, climate and picking.
    pub fn snapshot(&self) -> &AtlasTerrainSnapshot {
        self.artifact.snapshot()
    }
    /// Returns the verified presentation identity.
    pub fn presentation_source(&self) -> SphericalPresentationSource {
        self.source.clone()
    }
    /// Returns the original source topology.
    pub fn surface(&self) -> &SphericalSurfaceSnapshot {
        self.snapshot().surface()
    }
    /// Representative source-cell length in metres, from the mean area of the published cells.
    pub(super) const fn source_cell_spacing_m(&self) -> f64 {
        self.source_cell_spacing_m
    }
    /// Returns the actual C2 quality, independent of source cell count and display LOD.
    pub fn quality_profile(&self) -> NaturalQualityProfile {
        self.snapshot().climate().checkpoint().quality_profile()
    }
}

impl FieldDocument for AtlasFieldDocument {
    fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError> {
        let mut payloads = self.cache.payloads();
        payloads.extend([
            (
                crate::world::natural::potential_biome_field_id(),
                FieldPayloadRef::CategoryU32(self.snapshot().potential_biomes()),
            ),
            (
                surface_elevation_m_field_id(),
                FieldPayloadRef::ScalarF32(self.snapshot().fields().bed_elevation_m()),
            ),
            (
                land_ocean_field_id(),
                FieldPayloadRef::CategoryU32(self.snapshot().fields().land_ocean().raw_values()),
            ),
            (
                surface_water_kind_field_id(),
                FieldPayloadRef::CategoryU32(self.water.raw_values()),
            ),
            (
                circulation_surface_albedo_field_id(),
                FieldPayloadRef::ScalarF32(self.snapshot().climate().fields().surface_albedo()),
            ),
        ]);
        FieldCatalog::from_payloads(&self.registry, payloads)
    }
    fn diagnostics(&self) -> &[OwnedViewDiagnostic] {
        &self.diagnostics
    }
    fn preferred_field(&self) -> Option<FieldId> {
        Some(surface_elevation_m_field_id())
    }
    fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode> {
        super::spherical_formation_display::formation_preferred_range(
            &self.registry,
            0.0,
            Some(self.display_radius_m),
            field,
        )
    }
}

pub(super) fn show_summary(ui: &mut egui::Ui, document: &AtlasFieldDocument) {
    let snapshot = document.snapshot();
    let params = snapshot.fine_terrain().coarse_terrain().params();
    ui.strong(text::ATLAS_SUMMARY_LABEL);
    ui.label(text::atlas_source_cells(
        params.seed,
        document.surface().cells().len(),
        params.cells,
    ));
    ui.collapsing(text::ATLAS_SOURCE_DETAILS_LABEL, |ui| {
        ui.label(text::atlas_source_version(
            civ_atlas_terrain::UPSTREAM_PACKAGE_VERSION,
            civ_atlas_terrain::UPSTREAM_REVISION,
        ));
        let climate_resolution = document.quality_profile().climate_face_resolution();
        ui.label(text::atlas_source_scales(
            crate::world::natural::ATLAS_FINE_SCALE.pixels_per_unit(),
            document.source_cell_spacing_m(),
            climate_resolution,
            crate::generators::natural::climate_grid_spacing_m(
                document.surface().radius().get(),
                climate_resolution,
            ),
        ));
        ui.weak(text::ATLAS_DISPLAY_LOD_HINT);
        ui.label(text::atlas_source_water(
            snapshot.rivers().len(),
            snapshot.lakes().len(),
            snapshot.initial_water_inventory_m3(),
        ));
        if let Some((min, max)) = document.river_flow_range {
            ui.label(text::atlas_current_flow_range(min, max));
        }
        ui.weak(text::ATLAS_HYDROLOGY_HINT);
        ui.weak(text::ATLAS_PERFORMANCE_HINT);
    });
    ui.collapsing(text::ATLAS_MONTHLY_RUNOFF_LABEL, |ui| {
        let id = ui.make_persistent_id("atlas_budget_month");
        let mut month = ui.data(|data| data.get_temp::<usize>(id)).unwrap_or(1);
        if ui
            .add(
                egui::Slider::new(&mut month, 1..=crate::world::natural::CLIMATE_MONTH_COUNT)
                    .text(text::ATLAS_BUDGET_MONTH_LABEL),
            )
            .changed()
        {
            ui.data_mut(|data| data.insert_temp(id, month));
        }
        let rates = snapshot.runoff_budget().monthly_terms_m3_s(month - 1);
        egui::Grid::new("atlas_monthly_runoff").show(ui, |ui| {
            for (label, rate) in text::ATLAS_RUNOFF_TERMS.into_iter().zip(rates) {
                ui.label(label);
                ui.monospace(format!("{rate:.2}"));
                ui.end_row();
            }
        });
        ui.weak(text::ATLAS_MONTHLY_RUNOFF_HINT);
    });
    for metric in snapshot.climate_quality().metrics() {
        if metric.status() == crate::world::natural::QualityMetricStatus::Fail {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                crate::ui::field::localization::climate_quality_advisory_text(metric),
            );
        }
    }
}

pub(super) fn show_author_controls(
    ui: &mut egui::Ui,
    inputs: &mut super::authoring::AuthorInputs,
) -> bool {
    let mut focused = false;
    ui.weak(text::ATLAS_PERFORMANCE_HINT);
    ui.horizontal(|ui| {
        ui.label(text::ATLAS_INTEGER_LABELS[0]);
        focused |= ui
            .add(egui::DragValue::new(&mut inputs.atlas_shape.cells).update_while_editing(false))
            .has_focus();
    });
    egui::ComboBox::from_label(text::ATLAS_QUALITY_LABEL)
        .selected_text(text::atlas_climate_quality_label(
            inputs.formation_quality_profile,
        ))
        .show_ui(ui, |ui| {
            for profile in [
                NaturalQualityProfile::Draft,
                NaturalQualityProfile::Standard,
                NaturalQualityProfile::High,
            ] {
                ui.selectable_value(
                    &mut inputs.formation_quality_profile,
                    profile,
                    text::atlas_climate_quality_label(profile),
                );
            }
        });
    ui.collapsing(text::ATLAS_AUTHOR_DETAILS_LABEL, |ui| {
        ui.weak(text::ATLAS_AUTHOR_HINT);
        let p = &mut inputs.atlas_shape;
        egui::Grid::new("atlas_author_controls")
            .num_columns(2)
            .show(ui, |ui| {
                for (label, value) in [
                    (text::ATLAS_INTEGER_LABELS[1], &mut p.plates),
                    (text::ATLAS_INTEGER_LABELS[2], &mut p.continents),
                ] {
                    ui.label(label);
                    focused |= ui
                        .add(egui::DragValue::new(value).update_while_editing(false))
                        .has_focus();
                    ui.end_row();
                }
                for (label, value) in [
                    (text::ATLAS_SHAPE_LABELS[0], &mut p.land_fraction),
                    (text::ATLAS_SHAPE_LABELS[1], &mut p.mountains),
                    (text::ATLAS_SHAPE_LABELS[2], &mut p.temperature),
                    (text::ATLAS_SHAPE_LABELS[3], &mut p.rainfall),
                    (text::ATLAS_SHAPE_LABELS[4], &mut p.gather),
                    (text::ATLAS_SHAPE_LABELS[5], &mut p.rugged),
                    (text::ATLAS_SHAPE_LABELS[6], &mut p.hardness),
                    (text::ATLAS_SHAPE_LABELS[7], &mut p.north),
                ] {
                    ui.label(label);
                    focused |= ui
                        .add(
                            egui::DragValue::new(value)
                                .speed(0.01)
                                .update_while_editing(false),
                        )
                        .has_focus();
                    ui.end_row();
                }
            });
        ui.weak(text::ATLAS_SCOPE_HINT);
    });
    focused
}
