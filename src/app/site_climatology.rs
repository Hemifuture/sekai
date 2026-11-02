//! Site-climatology panel: region authoring, anchor recommendation, the
//! monthly table, the footprint overlay and the export action
//! (`docs/superpowers/specs/2026-10-26-site-climatology-export-design.md` §7).
//!
//! The app only assembles published products into the read-only query and
//! writes the bytes; sampling lives in `generators::natural::site_climatology`.

#[cfg(all(test, not(debug_assertions)))]
use crate::app::SphericalFormationFieldDocument;
use crate::app::SphericalWorldFieldDocument;
use crate::generators::natural::{
    recommend_site_anchors, sample_site_climatology, SiteAnchorCandidate, SiteClimatologyInputs,
    SiteFootprint,
};
use crate::ui::field::localization::{
    site_capability_label, site_capability_reason_label, SITE_CLIMATOLOGY_HEADING,
    SITE_EXPORT_LABEL, SITE_FINGERPRINT_LABEL, SITE_FORMATION_ONLY_HINT, SITE_HALF_EXTENT_LABEL,
    SITE_HEADING_LABEL, SITE_MONTH_COLUMNS, SITE_NO_ANCHOR_HINT, SITE_NO_CANDIDATE_HINT,
    SITE_RECOMMEND_LABEL, SITE_SCALE_LABEL, SITE_UNAVAILABLE_LABEL, SITE_USE_CANDIDATE_LABEL,
    SITE_USE_SELECTION_LABEL,
};
use crate::view::SelectedSurfaceEntity;
use crate::world::natural::{
    site_climatology_fingerprint, SiteAvailability, SiteClimatology, SiteClimatologySource,
    SiteRegion, QINGLAN_HALF_EXTENT_M,
};
use crate::world::spatial::UnitVector3;
use crate::world::CellId;

/// Points per footprint side in the overlay; gnomonic sides are great-circle
/// arcs, so a few samples keep map-projection curvature visible.
const OUTLINE_SAMPLES_PER_SIDE: usize = 8;

/// Directory, relative to the working directory, that native exports go to.
#[cfg(not(target_arch = "wasm32"))]
const EXPORT_DIRECTORY: &str = "site-climatology";

#[derive(Debug, Clone, Copy, PartialEq)]
struct RegionInputs {
    anchor: CellId,
    heading_deg: f64,
    scale: f64,
    half_extent_m: f64,
}

struct SampledSite {
    inputs: RegionInputs,
    export: SiteClimatology,
    bytes: Vec<u8>,
    fingerprint_hex: String,
    effective_outline: Vec<UnitVector3>,
    requested_outline: Option<Vec<UnitVector3>>,
}

/// Session-only panel state; it is reset whenever the published world changes.
pub(crate) struct SiteClimatologyPanel {
    world: Option<[u8; 32]>,
    heading_deg: f64,
    scale: f64,
    half_extent_m: f64,
    anchor: Option<CellId>,
    candidates: Option<Vec<SiteAnchorCandidate>>,
    sampled: Option<Result<SampledSite, String>>,
    message: Option<String>,
}

impl Default for SiteClimatologyPanel {
    fn default() -> Self {
        Self {
            world: None,
            heading_deg: 0.0,
            scale: 1.0,
            half_extent_m: QINGLAN_HALF_EXTENT_M,
            anchor: None,
            candidates: None,
            sampled: None,
            message: None,
        }
    }
}

fn inputs(world: &SphericalWorldFieldDocument) -> Option<SiteClimatologyInputs<'_>> {
    if let Some(document) = world.atlas() {
        return Some(SiteClimatologyInputs {
            surface: document.surface(),
            climate: document.snapshot().climate(),
            surface_water: &document.water,
            elevation_m: document.snapshot().fields().exposed_elevation_m(),
            climate_face_resolution: document.quality_profile().climate_face_resolution(),
        });
    }
    let document = world.formation()?;
    let formation = document.formation_snapshot();
    Some(SiteClimatologyInputs {
        surface: document.surface(),
        climate: document.formation_climate(),
        surface_water: formation.hydrology().surface_water(),
        elevation_m: formation.terrain_fields().current_elevation_m(),
        climate_face_resolution: document.quality_profile().climate_face_resolution(),
    })
}

fn source(world: &SphericalWorldFieldDocument) -> SiteClimatologySource {
    let presentation = world.presentation_source();
    let climate = inputs(world)
        .expect("supported current-climate document")
        .climate;
    let hex = |bytes: [u8; 32]| blake3::Hash::from(bytes).to_hex().to_string();
    SiteClimatologySource {
        world_seed: presentation.root_seed().raw(),
        surface_fingerprint: hex(presentation.surface_ref().fingerprint()),
        build_result_hash: hex(*presentation.build_result_hash().as_bytes()),
        graph_contract_version: presentation.graph_contract_version(),
        global_circulation_fingerprint: hex(climate.fields().fingerprint()),
        global_circulation_schema: climate.schema_version(),
        climate_profile: climate.profile(),
        quality_profile: climate.checkpoint().quality_profile(),
        terrain_source: if world.atlas().is_some() {
            crate::world::natural::SiteTerrainSource::CivAtlas
        } else {
            crate::world::natural::SiteTerrainSource::Formation
        },
        fixed_terrain_scale: world
            .atlas()
            .map(|_| crate::world::natural::ATLAS_FINE_SCALE.pixels_per_unit()),
        sekai_package_version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

fn sample(
    world: &SphericalWorldFieldDocument,
    region_inputs: RegionInputs,
) -> Result<SampledSite, String> {
    let query = inputs(world).ok_or("This source has no current climate")?;
    let region = SiteRegion::new(
        region_inputs.anchor,
        region_inputs.heading_deg,
        region_inputs.scale,
        region_inputs.half_extent_m,
    )
    .map_err(|error| error.to_string())?;
    let export = sample_site_climatology(&query, &region, source(world))
        .map_err(|error| error.to_string())?;
    let footprint = SiteFootprint::new(&query, &region).map_err(|error| error.to_string())?;
    let bytes = export.to_json_bytes();
    let fingerprint_hex = blake3::Hash::from(site_climatology_fingerprint(&bytes))
        .to_hex()
        .to_string();
    Ok(SampledSite {
        inputs: region_inputs,
        export,
        bytes,
        fingerprint_hex,
        effective_outline: footprint
            .outline(footprint.effective_side_m(), OUTLINE_SAMPLES_PER_SIDE),
        requested_outline: footprint
            .expanded()
            .then(|| footprint.outline(footprint.requested_side_m(), OUTLINE_SAMPLES_PER_SIDE)),
    })
}

impl SiteClimatologyPanel {
    /// Retires world-bound samples even when the panel is collapsed or publication
    /// happens after controls were drawn; authoring preferences remain session-local.
    pub(super) fn synchronize_world(&mut self, identity: Option<[u8; 32]>) {
        if self.world != identity {
            *self = Self {
                world: identity,
                heading_deg: self.heading_deg,
                scale: self.scale,
                half_extent_m: self.half_extent_m,
                ..Self::default()
            };
        }
    }

    fn region_inputs(&self) -> Option<RegionInputs> {
        self.anchor.map(|anchor| RegionInputs {
            anchor,
            heading_deg: self.heading_deg,
            scale: self.scale,
            half_extent_m: self.half_extent_m,
        })
    }

    /// Effective footprint outline, plus the requested one when it was widened.
    pub(crate) fn outlines(&self) -> Option<(&[UnitVector3], Option<&[UnitVector3]>)> {
        let Some(Ok(site)) = &self.sampled else {
            return None;
        };
        Some((&site.effective_outline, site.requested_outline.as_deref()))
    }

    /// Draws the panel for the published world and the current selection.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        world: &SphericalWorldFieldDocument,
        selection: Option<SelectedSurfaceEntity>,
    ) {
        self.synchronize_world(Some(
            *world.presentation_source().build_result_hash().as_bytes(),
        ));
        ui.separator();
        ui.collapsing(SITE_CLIMATOLOGY_HEADING, |ui| {
            let Some(_) = inputs(world) else {
                ui.label(SITE_FORMATION_ONLY_HINT);
                return;
            };
            self.show_region_controls(ui, selection);
            self.show_recommendations(ui, world);
            let Some(region_inputs) = self.region_inputs() else {
                ui.weak(SITE_NO_ANCHOR_HINT);
                return;
            };
            if !matches!(&self.sampled, Some(Ok(site)) if site.inputs == region_inputs) {
                self.sampled = Some(sample(world, region_inputs));
                self.message = None;
            }
            match &self.sampled {
                Some(Ok(site)) => {
                    show_site(ui, &site.export);
                    self.show_export(ui);
                }
                Some(Err(error)) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                None => {}
            }
        });
    }

    fn show_region_controls(
        &mut self,
        ui: &mut egui::Ui,
        selection: Option<SelectedSurfaceEntity>,
    ) {
        egui::Grid::new("site_region")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(SITE_HEADING_LABEL);
                ui.add(
                    egui::DragValue::new(&mut self.heading_deg)
                        .range(-180.0..=180.0)
                        .speed(1.0),
                );
                ui.end_row();
                ui.label(SITE_SCALE_LABEL);
                ui.add(
                    egui::DragValue::new(&mut self.scale)
                        .range(0.01..=1_000.0)
                        .speed(0.1),
                );
                ui.end_row();
                ui.label(SITE_HALF_EXTENT_LABEL);
                ui.add(
                    egui::DragValue::new(&mut self.half_extent_m)
                        .range(1.0..=10_000_000.0)
                        .speed(64.0),
                );
                ui.end_row();
            });
        let selected_cell = match selection {
            Some(SelectedSurfaceEntity::Cell(cell)) => Some(cell),
            _ => None,
        };
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    selected_cell.is_some(),
                    egui::Button::new(SITE_USE_SELECTION_LABEL),
                )
                .clicked()
            {
                self.anchor = selected_cell;
            }
            if let Some(anchor) = self.anchor {
                ui.label(format!("Cell {}", anchor.raw()));
            }
        });
    }

    fn show_recommendations(&mut self, ui: &mut egui::Ui, world: &SphericalWorldFieldDocument) {
        if ui.button(SITE_RECOMMEND_LABEL).clicked() {
            let region = SiteRegion::new(
                CellId::from_raw(0),
                self.heading_deg,
                self.scale,
                self.half_extent_m,
            );
            self.candidates = region
                .and_then(|region| {
                    recommend_site_anchors(&inputs(world).expect("supported document"), &region)
                })
                .map_err(|error| self.message = Some(error.to_string()))
                .ok();
        }
        let Some(candidates) = &self.candidates else {
            return;
        };
        if candidates.is_empty() {
            ui.weak(SITE_NO_CANDIDATE_HINT);
        }
        let mut chosen = None;
        for candidate in candidates {
            let centroid = world.surface().cells()[candidate.cell.raw() as usize].centroid;
            let (latitude, longitude) = crate::world::spatial::latitude_longitude_radians(centroid);
            let assessment = &candidate.assessment;
            ui.horizontal(|ui| {
                if ui.small_button(SITE_USE_CANDIDATE_LABEL).clicked() {
                    chosen = Some(candidate.cell);
                }
                ui.label(format!(
                    "{:+.1}°, {:+.1}°｜起伏 {:.0} m｜离岸 {}",
                    latitude.to_degrees(),
                    longitude.to_degrees(),
                    assessment.footprint_relief_m,
                    format_distance(assessment.coast_distance_m),
                ));
            });
        }
        if chosen.is_some() {
            self.anchor = chosen;
        }
    }

    fn show_export(&mut self, ui: &mut egui::Ui) {
        let Some(Ok(site)) = &self.sampled else {
            return;
        };
        ui.label(format!(
            "{SITE_FINGERPRINT_LABEL}：{}",
            site.fingerprint_hex
        ));
        if ui.button(SITE_EXPORT_LABEL).clicked() {
            self.message = Some(write_export(ui.ctx(), &site.bytes, &site.fingerprint_hex));
        }
        if let Some(message) = &self.message {
            ui.label(message);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_export(_ctx: &egui::Context, bytes: &[u8], fingerprint_hex: &str) -> String {
    let path = std::path::Path::new(EXPORT_DIRECTORY)
        .join(format!("site-climatology-{}.json", &fingerprint_hex[..16]));
    match std::fs::create_dir_all(EXPORT_DIRECTORY).and_then(|()| std::fs::write(&path, bytes)) {
        // `std::path::absolute`, not `canonicalize`: on Windows the latter
        // returns a verbatim `\\?\` path that the panel would show verbatim.
        Ok(()) => format!(
            "已写入 {}",
            std::path::absolute(&path).unwrap_or(path).display()
        ),
        Err(error) => format!("写入失败：{error}"),
    }
}

#[cfg(target_arch = "wasm32")]
fn write_export(ctx: &egui::Context, bytes: &[u8], _fingerprint_hex: &str) -> String {
    ctx.copy_text(String::from_utf8_lossy(bytes).into_owned());
    "已复制 JSON 到剪贴板".to_owned()
}

fn format_distance(distance_m: Option<f64>) -> String {
    distance_m.map_or_else(
        || "无海洋".to_owned(),
        |distance| format!("{:.0} km", distance / 1_000.0),
    )
}

fn show_site(ui: &mut egui::Ui, site: &SiteClimatology) {
    let region = &site.region;
    let assessment = &site.anchor_assessment;
    ui.label(format!(
        "锚点 {:+.2}°, {:+.2}°｜取样边长 {:.0} km{}｜{} 格",
        region.anchor_lat_deg,
        region.anchor_lon_deg,
        region.effective_footprint_side_m / 1_000.0,
        if region.footprint_expanded {
            format!(
                "（请求 {:.1} km，已扩大到 P4 格距）",
                region.requested_footprint_side_m / 1_000.0
            )
        } else {
            String::new()
        },
        region.cell_count,
    ));
    ui.label(format!(
        "陆地 {:.0}%｜湖泊 {:.0}%｜高程 {:.0}–{:.0} m（均值 {:.0} m）",
        region.land_fraction * 100.0,
        region.lake_fraction * 100.0,
        region.elevation_m.min,
        region.elevation_m.max,
        region.elevation_m.mean,
    ));
    ui.label(format!(
        "离岸 {}（内陆门槛 {:.0} km）｜{}",
        format_distance(assessment.coast_distance_m),
        assessment.inland_threshold_m / 1_000.0,
        if assessment.meets_criteria {
            "满足内陆选址条件"
        } else {
            "不满足内陆选址条件"
        },
    ));
    for status in &site.capabilities {
        if status.availability == SiteAvailability::Available {
            continue;
        }
        let reason = status.reason.map_or("", site_capability_reason_label);
        ui.weak(format!(
            "{}：{SITE_UNAVAILABLE_LABEL}（{reason}）",
            site_capability_label(status.id)
        ));
    }
    let dash = || "—".to_owned();
    egui::Grid::new("site_months")
        .striped(true)
        .num_columns(SITE_MONTH_COLUMNS.len())
        .show(ui, |ui| {
            for column in SITE_MONTH_COLUMNS {
                ui.strong(column);
            }
            ui.end_row();
            for month in &site.months {
                let steering = month
                    .steering_wind_en_m_s
                    .unwrap_or(month.near_surface_wind_en_m_s);
                let orographic_share = (month.precipitation_mm_day > 0.0)
                    .then(|| month.orographic_precipitation_mm_day / month.precipitation_mm_day);
                ui.label(month.month.to_string());
                ui.label(format!("{:.1}", month.air_temperature_c));
                ui.label(format!("{:.2}", month.precipitation_mm_day));
                ui.label(
                    orographic_share.map_or_else(dash, |share| format!("{:.0}%", share * 100.0)),
                );
                ui.label(format!("{:.0}%", month.relative_humidity * 100.0));
                ui.label(format!("{:.1}", steering[0].hypot(steering[1])));
                ui.label(format!("{:.0}", month.steering_flow_toward_bearing_deg));
                ui.label(
                    month
                        .eady_growth_rate_per_day
                        .map_or_else(dash, |rate| format!("{rate:.2}")),
                );
                ui.label(
                    month
                        .wet_day_fraction
                        .map_or_else(dash, |fraction| format!("{fraction:.2}")),
                );
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(debug_assertions))]
    use crate as coast_probe_crate;
    #[cfg(not(debug_assertions))]
    mod coast_evidence {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/procedural_coast.rs"
        ));
    }

    /// One final-world solve covers the changed work grid, fine coasts and the actual
    /// UI sampling adapter together; no second world or copied source-stamp builder.
    #[cfg(not(debug_assertions))]
    #[test]
    #[ignore = "offline integrated Draft coast and current site-export identity"]
    fn integrated_procedural_coast_site_uses_published_identity() {
        use crate::world::natural::{NaturalQualityProfile, SurfaceWaterKind};
        let product = coast_evidence::generate_procedural_coast_preview(
            NaturalQualityProfile::Draft,
            coast_evidence::target25_relief(),
            "integration-target25.json",
            crate::generators::natural::causal_natural_formation_graph().unwrap(),
        );
        let document = std::sync::Arc::new(
            SphericalFormationFieldDocument::from_build_outcome(&product).unwrap(),
        );
        let world = SphericalWorldFieldDocument::Formation(std::sync::Arc::clone(&document));
        // Select by stable id order, not by desirable climate or an inland-score search.
        let water = document.formation_snapshot().hydrology().surface_water();
        let anchor = document
            .surface()
            .cells()
            .iter()
            .find(|cell| water.get(cell.id.raw() as usize) == Some(SurfaceWaterKind::DryLand))
            .expect("the target-land fixture contains land")
            .id;
        let panel = SiteClimatologyPanel {
            anchor: Some(anchor),
            ..Default::default()
        };
        let sampled = sample(&world, panel.region_inputs().unwrap()).unwrap();
        assert_eq!(
            sampled.export.source.build_result_hash,
            blake3::Hash::from(*world.presentation_source().build_result_hash().as_bytes())
                .to_hex()
                .to_string()
        );
        assert_eq!(
            sampled.export.source.global_circulation_fingerprint,
            blake3::Hash::from(document.formation_climate().fields().fingerprint())
                .to_hex()
                .to_string()
        );
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/site-climatology-integration");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("current-procedural-site.json"),
            &sampled.bytes,
        )
        .unwrap();
        eprintln!(
            "SITE_EXPORT_INTEGRATION fingerprint={} build={}",
            sampled.fingerprint_hex, sampled.export.source.build_result_hash
        );
    }

    // Publication can happen after the collapsed panel was drawn. This state-level
    // contract catches stale overlays without a GPU or another climate solve.
    #[test]
    fn publication_retires_site_cache_without_opening_the_panel() {
        let mut panel = SiteClimatologyPanel {
            world: Some([1; 32]),
            anchor: Some(CellId::from_raw(7)),
            heading_deg: 17.0,
            candidates: Some(Vec::new()),
            message: Some("old export".to_owned()),
            // The cached result owns either an error or a sampled footprint. Testing
            // disposal of this slot needs neither a climate solve nor a new decoder.
            sampled: Some(Err("old sample".to_owned())),
            ..Default::default()
        };
        let preferences = (panel.heading_deg, panel.scale, panel.half_extent_m);
        panel.synchronize_world(Some([1; 32]));
        assert!(
            panel.sampled.is_some(),
            "same-world refresh retains the sample"
        );
        panel.synchronize_world(Some([2; 32]));
        assert!(panel.sampled.is_none());
        assert!(panel.anchor.is_none() && panel.candidates.is_none() && panel.message.is_none());
        assert_eq!(
            (panel.heading_deg, panel.scale, panel.half_extent_m),
            preferences
        );
        panel.synchronize_world(None);
        assert!(panel.world.is_none());
    }
}
