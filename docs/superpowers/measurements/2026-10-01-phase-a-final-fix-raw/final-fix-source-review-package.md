BASE=516c23c64eddb7f083caf4ab4a8d2eb6d6805bdf
HEAD=50e379cfe2b918bdd8a50445d318a7a7f082d13d
Complete source/test/Cargo diff. Full evidence diff is in whole-branch-pre-ui-package.md.

50e379c Fix authoring number confirmation and show the current build phase

 src/app.rs                        | 589 ++++++++++++++++++++++++++++++++++++--
 src/app/project_ui.rs             |   3 +
 src/app/spherical_presentation.rs |  12 +-
 3 files changed, 575 insertions(+), 29 deletions(-)

diff --git a/src/app.rs b/src/app.rs
index 0b4bdd5..11434b3 100644
--- a/src/app.rs
+++ b/src/app.rs
@@ -156,23 +156,50 @@ fn formation_authoring_control_state(
 }
 
 /// One cached formation profile surface keyed by tier and authored radius.
 #[derive(Clone)]
 struct FormationSurfaceCacheEntry {
     profile: crate::world::natural::NaturalQualityProfile,
     radius_m: f64,
     surface: crate::world::spatial::SphericalSurfaceSnapshot,
 }
 
+/// Current coarse operation of one request; never persisted or a solver progress estimate.
+#[derive(Debug, Clone, Copy, PartialEq, Eq)]
+enum WorldBuildPhase {
+    Surface,
+    Graph,
+    Presentation,
+}
+
+impl WorldBuildPhase {
+    fn label(self) -> &'static str {
+        match self {
+            Self::Surface => "地表准备",
+            Self::Graph => "自然世界构建",
+            Self::Presentation => "显示准备/发布",
+        }
+    }
+
+    fn enter(self, current: Option<&std::sync::Mutex<Self>>) {
+        if let Some(current) = current {
+            *current
+                .lock()
+                .expect("phase lock is never held during work") = self;
+        }
+    }
+}
+
 /// A spherical world build running on a worker thread.
 struct PendingWorldBuild {
     token: BuildToken,
+    phase: Arc<std::sync::Mutex<WorldBuildPhase>>,
     worker: Option<std::thread::JoinHandle<()>>,
     receiver: std::sync::mpsc::Receiver<WorldBuildCompletion>,
     cancellation: crate::engine::BuildCancellation,
     started_at: std::time::Instant,
     replacement: bool,
     retained_stage_cache: Option<MemoryStageCache>,
     /// The finished worker payload together with the egui pass that parked
     /// it. Settlement is refused inside that same pass, so the cancel button
     /// is always drawn at least once between reception and publication.
     ///
@@ -565,20 +592,21 @@ fn prepare_pending_world_build(
     let (token, cancellation) = request;
     let (working_stage_cache, retained_stage_cache) = if pipeline == WorldPipeline::Formation {
         (published_stage_cache.clone(), Some(published_stage_cache))
     } else {
         (published_stage_cache, None)
     };
     (
         working_stage_cache,
         PendingWorldBuild {
             token,
+            phase: Arc::new(std::sync::Mutex::new(WorldBuildPhase::Surface)),
             worker: None,
             receiver,
             cancellation,
             started_at: std::time::Instant::now(),
             replacement,
             retained_stage_cache,
             completion: None,
         },
     )
 }
@@ -607,25 +635,34 @@ enum MigrationFailurePoint {
 
 fn configure_frame_stats_scenario(canvas: &mut crate::ui::spherical::SphericalCanvasState) {
     let mut state = canvas.field_state().clone();
     state.select_fill(surface_elevation_m_field_id());
     state.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
     state.set_vector_lod(VectorGlyphLod::Medium);
     state.select_entity(None);
     canvas.replace_field_state(state);
 }
 
+/// Rejected text keeps its numeric-editor ID when Slider returns its track ID after blur.
+#[derive(Debug, Clone)]
+struct RejectedAuthorInput {
+    field: &'static str,
+    widget_id: egui::Id,
+    text: String,
+}
+
 /// A committed authoring session and the current in-memory display resources.
 pub struct TemplateApp {
     world_origin: PersistedWorldOrigin,
     authoring: AuthoringSession,
     input_edit: Option<AuthorInputs>,
+    author_input_error: Option<RejectedAuthorInput>,
     #[cfg(not(target_arch = "wasm32"))]
     project_ui: project_ui::ProjectUiState,
     spherical_canvas_state: crate::ui::spherical::SphericalCanvasState,
     reduce_loading_motion: bool,
     formation_surface: Option<FormationSurfaceCacheEntry>,
     amplified_mesh: Option<std::sync::Arc<crate::view::AmplifiedSurfaceMesh>>,
     amplified_detail: Option<AmplifiedDetailEngine>,
     river_polylines: Option<std::sync::Arc<Vec<crate::view::RiverPolylineSegment>>>,
     river_radius_m: Option<f64>,
     /// The active mesh's pre-projected map geometry (worker-produced;
@@ -776,20 +813,21 @@ impl<'de> serde::Deserialize<'de> for TemplateApp {
     }
 }
 
 impl Default for TemplateApp {
     fn default() -> Self {
         Self {
             world_origin: PersistedWorldOrigin::SphericalV1,
             authoring: AuthoringSession::new(AuthorInputs::default())
                 .expect("default author inputs satisfy domain contracts"),
             input_edit: None,
+            author_input_error: None,
             #[cfg(not(target_arch = "wasm32"))]
             project_ui: project_ui::ProjectUiState::default(),
             spherical_canvas_state: crate::ui::spherical::SphericalCanvasState::default(),
             reduce_loading_motion: false,
             amplified_mesh: None,
             amplified_map_projected: None,
             amplified_detail: None,
             river_polylines: None,
             river_radius_m: None,
             formation_surface: None,
@@ -1387,20 +1425,21 @@ impl TemplateApp {
             stage_cache,
             receiver,
             (token, cancellation),
             is_replacement,
         );
         self.build_observation = self
             .frame_sampler
             .is_requested_or_enabled()
             .then(|| BuildObservation::new(token, request_started));
         let observation = self.build_observation.clone();
+        let worker_phase = pending_build.phase.clone();
         if observation.is_some() {
             if let Some(timing) = origin {
                 emit_runtime_line(&format!("SEKAI_BUILD_ORIGIN token={token:?} origin=open read_and_decode_s={:.6} pre_worker_s={:.6} includes_compatibility_confirmation_and_previous_worker_wait=true", timing.read_and_decode.as_secs_f64(), request_started.elapsed().as_secs_f64()));
             } else {
                 emit_runtime_line(&format!(
                     "SEKAI_BUILD_ORIGIN token={token:?} origin=build_request"
                 ));
             }
         }
         let worker = std::thread::spawn(move || {
@@ -1466,20 +1505,21 @@ impl TemplateApp {
                         tectonic: tectonic_spec,
                         relief: relief_spec,
                         geologic: geologic_spec,
                         view_state,
                         requested_state,
                         replacement,
                     },
                     &mut stage_cache,
                     &worker_cancellation,
                     observation.as_ref(),
+                    Some(&worker_phase),
                 )
                 .map_err(|error| {
                     // Surface the report's error diagnostics: "inspect the
                     // report diagnostics" is useless if nothing displays them.
                     let mut message = error.to_string();
                     if let crate::app::SphericalPresentationError::Build(failure) = &error {
                         for diagnostic in failure.report.diagnostics().iter().filter(|diagnostic| {
                             diagnostic.severity() == crate::engine::DiagnosticSeverity::Error
                         }) {
                             message.push('\n');
@@ -1582,26 +1622,31 @@ impl TemplateApp {
                 observation
                     .cancel_feedback_at
                     .get_or_insert(observation.started_at.elapsed());
             }
         }
         let mut cancel_rect = None;
         let mut cancel_requested = false;
         ui.horizontal(|ui| {
             ui.add(egui::Spinner::new());
             ui.label(format!(
-                "{}…已用 {:.0} 秒",
+                "{} · {}…已用 {:.0} 秒",
                 if pending.cancellation.is_cancelled() {
                     "正在取消"
                 } else {
                     "正在生成世界"
                 },
+                pending
+                    .phase
+                    .lock()
+                    .expect("phase lock is never held during work")
+                    .label(),
                 pending.started_at.elapsed().as_secs_f32()
             ));
             let cancel = ui.button("取消");
             cancel_rect = Some(cancel.rect);
             // `clicked()` only fires on the release pass, so a user pressing
             // and holding 取消 while the worker's completion is staged in that
             // same pass would be published over. Pointer-down cancels at once;
             // `clicked()` keeps keyboard activation working.
             if cancel.clicked() || cancel.is_pointer_button_down_on() {
                 cancel_requested = true;
@@ -2461,44 +2506,54 @@ impl eframe::App for TemplateApp {
                 egui::ScrollArea::vertical().show(ui, |ui| {
                     ui.heading("自然世界");
                     ui.label(CURRENT_SLICE_SUBTITLE);
                     ui.separator();
 
                     ui.horizontal(|ui| {
                         ui.label("根种子");
                         let rejected_text = std::cell::RefCell::new(None);
                         let parsed_seed = std::cell::Cell::new(None);
                         let exact_seed = editing.world_seed;
+                        let rejected_seed =
+                            rejected_authoring_text(&self.author_input_error, "根种子");
                         let response = ui.add(
                             egui::DragValue::new(&mut editing.world_seed)
                                 .range(0..=u64::MAX)
-                                .custom_formatter(move |_, _| exact_seed.to_string())
+                                .custom_formatter(move |_, _| {
+                                    rejected_seed
+                                        .clone()
+                                        .unwrap_or_else(|| exact_seed.to_string())
+                                })
                                 .custom_parser(|text| {
                                     match normalized_authoring_number(text).parse::<u64>() {
                                         Ok(seed) => {
                                             parsed_seed.set(Some(seed));
                                             Some(seed as f64)
                                         }
                                         Err(_) => {
                                             rejected_text.replace(Some(text.to_owned()));
                                             None
                                         }
                                     }
                                 })
                                 .update_while_editing(false),
                         );
                         // DragValue's f64 is only a UI bridge; commit the exact u64 text.
                         if let Some(seed) = parsed_seed.get() {
                             editing.world_seed = seed;
                         }
-                        author_input_focused |=
-                            authoring_number_stays_pending(&response, rejected_text.into_inner());
+                        author_input_focused |= authoring_number_stays_pending(
+                            &response,
+                            rejected_text.into_inner(),
+                            &mut self.author_input_error,
+                            "根种子",
+                        );
                     });
                     if ui.button("新种子并重建").clicked() {
                         new_seed = true;
                     }
 
                     ui.add_space(8.0);
                     let previous_preset = editing.formation_spec.preset;
                     egui::ComboBox::from_label("世界形态")
                         .selected_text(formation_preset_label(editing.formation_spec.preset))
                         .show_ui(ui, |ui| {
@@ -2521,29 +2576,44 @@ impl eframe::App for TemplateApp {
                         let selected = editing.formation_spec.preset;
                         apply_formation_preset_selection(
                             &mut editing.formation_spec,
                             &mut editing.tectonic_spec,
                             &mut editing.relief_spec,
                             selected,
                         );
                     }
                     ui.horizontal(|ui| {
                         ui.label(INITIAL_PLATE_COUNT_LABEL);
+                        let rejected_plates = rejected_authoring_text(
+                            &self.author_input_error,
+                            INITIAL_PLATE_COUNT_LABEL,
+                        );
                         let rejected_text = std::cell::RefCell::new(None);
                         let response = ui.add(
                             egui::DragValue::new(&mut editing.tectonic_spec.plate_count)
                                 .range(MIN_PLATE_COUNT..=MAX_PLATE_COUNT)
-                                .custom_parser(|text| parse_authoring_number(text, &rejected_text))
+                                .custom_formatter(move |value, _| {
+                                    rejected_plates
+                                        .clone()
+                                        .unwrap_or_else(|| (value as u16).to_string())
+                                })
+                                .custom_parser(|text| {
+                                    parse_authoring_plate_count(text, &rejected_text)
+                                })
                                 .update_while_editing(false),
                         );
-                        author_input_focused |=
-                            authoring_number_stays_pending(&response, rejected_text.into_inner());
+                        author_input_focused |= authoring_number_stays_pending(
+                            &response,
+                            rejected_text.into_inner(),
+                            &mut self.author_input_error,
+                            "初始板块数",
+                        );
                     });
                     if editing.world_pipeline == WorldPipeline::Formation {
                         ui.horizontal(|ui| {
                             ui.label("驱动");
                             ui.radio_value(
                                 &mut editing.relief_spec.sea_level_policy,
                                 SeaLevelPolicy::WaterInventory,
                                 "陆壳比例",
                             )
                             .on_hover_text("物理解：海平面由表层水量与地形共同决定");
@@ -2554,63 +2624,103 @@ impl eframe::App for TemplateApp {
                             )
                             .on_hover_text("按目标陆地占比求解海平面，并推算所需海水量");
                         });
                     }
                     let controls = formation_authoring_control_state(
                         editing.world_pipeline,
                         &editing.relief_spec,
                         published_area_summary,
                     );
                     if controls.land_fraction_enabled {
+                        let current_fraction = editing.relief_spec.target_land_fraction;
+                        let rejected_fraction =
+                            rejected_authoring_text(&self.author_input_error, "陆地占比");
                         let rejected_text = std::cell::RefCell::new(None);
                         let response = ui.add(
                             egui::Slider::new(
                                 &mut editing.relief_spec.target_land_fraction,
                                 crate::world::natural::MIN_TARGET_LAND_FRACTION
                                     ..=crate::world::natural::MAX_TARGET_LAND_FRACTION,
                             )
                             .text("陆地占比")
-                            .custom_parser(|text| parse_authoring_number(text, &rejected_text))
-                            .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
+                            .custom_parser(|text| {
+                                parse_authoring_percentage(
+                                    text,
+                                    current_fraction,
+                                    crate::world::natural::MIN_TARGET_LAND_FRACTION
+                                        ..=crate::world::natural::MAX_TARGET_LAND_FRACTION,
+                                    &rejected_text,
+                                )
+                            })
+                            .custom_formatter(move |value, _| {
+                                rejected_fraction
+                                    .clone()
+                                    .unwrap_or_else(|| format_authoring_percentage(value))
+                            }),
+                        );
+                        author_input_focused |= authoring_number_stays_pending(
+                            &response,
+                            rejected_text.into_inner(),
+                            &mut self.author_input_error,
+                            "陆地占比",
                         );
-                        author_input_focused |=
-                            authoring_number_stays_pending(&response, rejected_text.into_inner());
                     } else {
                         let mut measured_land_fraction = controls.displayed_land_fraction;
                         ui.add_enabled(
                             false,
                             egui::Slider::new(&mut measured_land_fraction, 0.0..=1.0)
                                 .text("陆地占比（上次构建实测）")
                                 .custom_formatter(|value, _| format!("{:.1}%", value * 100.0)),
                         )
                         .on_disabled_hover_text("陆壳比例驱动时，陆地占比由物理水线推算");
                     }
                     ui.collapsing("高级", |ui| {
+                        let current_fraction = editing.tectonic_spec.continental_crust_fraction;
+                        let rejected_fraction =
+                            rejected_authoring_text(&self.author_input_error, "初始大陆地壳比例");
                         let rejected_text = std::cell::RefCell::new(None);
                         let response = ui
                             .add_enabled(
                                 controls.continental_fraction_enabled,
                                 egui::Slider::new(
                                     &mut editing.tectonic_spec.continental_crust_fraction,
                                     MIN_CONTINENTAL_CRUST_FRACTION..=MAX_CONTINENTAL_CRUST_FRACTION,
                                 )
                                 .text(if controls.continental_fraction_enabled {
                                     "初始大陆地壳比例"
                                 } else {
                                     "初始大陆地壳比例（预设值）"
                                 })
-                                .custom_parser(|text| parse_authoring_number(text, &rejected_text))
-                                .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
+                                .custom_parser(|text| {
+                                    parse_authoring_percentage(
+                                        text,
+                                        current_fraction,
+                                        MIN_CONTINENTAL_CRUST_FRACTION
+                                            ..=MAX_CONTINENTAL_CRUST_FRACTION,
+                                        &rejected_text,
+                                    )
+                                })
+                                .custom_formatter(
+                                    move |value, _| {
+                                        rejected_fraction
+                                            .clone()
+                                            .unwrap_or_else(|| format_authoring_percentage(value))
+                                    },
+                                ),
                             )
                             .on_disabled_hover_text("陆地占比驱动时，陆壳比例锁定为当前作者值");
-                        author_input_focused |=
-                            authoring_number_stays_pending(&response, rejected_text.into_inner());
+                        author_input_focused |= authoring_number_stays_pending(
+                            &response,
+                            rejected_text.into_inner(),
+                            &mut self.author_input_error,
+                            "初始大陆地壳比例",
+                        );
                     });
                     egui::ComboBox::from_label("构造活动")
                         .selected_text(activity_label(editing.tectonic_spec.activity))
                         .show_ui(ui, |ui| {
                             for activity in [
                                 TectonicActivity::Quiet,
                                 TectonicActivity::Moderate,
                                 TectonicActivity::Active,
                             ] {
                                 ui.selectable_value(
@@ -2646,20 +2756,29 @@ impl eframe::App for TemplateApp {
                         editing.world_seed = rand::random();
                         rebuild = true;
                     }
                     // Settle this frame's edit before drawing its save/map status.
                     self.finish_author_input(
                         editing,
                         author_input_focused,
                         ctx.input(|input| input.pointer.any_down()),
                         cancel_input_edit,
                     );
+                    if let Some(error) = &self.author_input_error {
+                        ui.colored_label(
+                            egui::Color32::LIGHT_RED,
+                            format!(
+                                "无效{} {}：请按控件单位输入范围内的数值",
+                                error.field, error.text
+                            ),
+                        );
+                    }
                     // Variable status rows follow the numeric editors so their
                     // auto IDs and pending text survive status changes.
                     #[cfg(not(target_arch = "wasm32"))]
                     self.show_project_status(ui, ctx);
 
                     self.spherical_presentation.read_resource(|current| {
                         let Some(presentation) = current.as_ref() else {
                             return;
                         };
                         ui.separator();
@@ -2752,22 +2871,26 @@ impl eframe::App for TemplateApp {
 impl TemplateApp {
     fn finish_author_input(
         &mut self,
         editing: AuthorInputs,
         focused: bool,
         pointer_down: bool,
         escape: bool,
     ) {
         self.input_edit = None;
         if escape {
+            self.author_input_error = None;
             return;
         }
+        if !focused {
+            self.author_input_error = None;
+        }
         if focused || pointer_down {
             if focused || editing != *self.authoring.draft() {
                 self.pending_open_build = None;
                 self.input_edit = Some(editing);
             }
         } else {
             match self
                 .authoring
                 .commit("编辑生成参数".to_owned(), editing.clone())
             {
@@ -2776,54 +2899,120 @@ impl TemplateApp {
                 Err(error) => {
                     self.pending_open_build = None;
                     self.input_edit = Some(editing);
                     self.spherical_runtime_error = Some(error.to_string());
                 }
             }
         }
     }
 }
 
-fn parse_authoring_number(
+fn parse_authoring_plate_count(
     text: &str,
     rejected: &std::cell::RefCell<Option<String>>,
 ) -> Option<f64> {
-    match normalized_authoring_number(text)
+    let value = normalized_authoring_number(text)
+        .parse::<u16>()
+        .ok()
+        .filter(|value| (MIN_PLATE_COUNT..=MAX_PLATE_COUNT).contains(value));
+    if value.is_none() {
+        rejected.replace(Some(text.to_owned()));
+    }
+    value.map(f64::from)
+}
+
+fn format_authoring_percentage(value: f64) -> String {
+    format!("{}%", value as f32 * 100.0)
+}
+
+fn parse_authoring_percentage(
+    text: &str,
+    current: f32,
+    range: std::ops::RangeInclusive<f32>,
+    rejected: &std::cell::RefCell<Option<String>>,
+) -> Option<f64> {
+    let normalized = normalized_authoring_number(text);
+    let number = normalized.strip_suffix('%').unwrap_or(&normalized);
+    let displayed = format_authoring_percentage(f64::from(current));
+    // A displayed percentage can lose one f32 bit on division. Keep the authored
+    // bits when confirming the displayed value so a no-op cannot create a recipe.
+    if number == displayed.trim_end_matches('%') {
+        return Some(f64::from(current));
+    }
+    let value = number
         .parse::<f64>()
         .ok()
-        .filter(|value| value.is_finite())
-    {
-        Some(value) => Some(value),
-        None => {
-            rejected.replace(Some(text.to_owned()));
-            None
-        }
+        .map(|percent| (percent / 100.0) as f32)
+        .filter(|fraction| fraction.is_finite() && range.contains(fraction));
+    if value.is_none() {
+        rejected.replace(Some(text.to_owned()));
     }
+    value.map(f64::from)
 }
 
 fn normalized_authoring_number(text: &str) -> String {
     text.chars()
         .filter(|c| !c.is_whitespace())
         .map(|c| if c == '−' { '-' } else { c })
         .collect()
 }
 
-fn authoring_number_stays_pending(response: &egui::Response, rejected: Option<String>) -> bool {
+fn rejected_authoring_text(error: &Option<RejectedAuthorInput>, field: &str) -> Option<String> {
+    error
+        .as_ref()
+        .filter(|error| error.field == field)
+        .map(|error| error.text.clone())
+}
+
+fn authoring_number_stays_pending(
+    response: &egui::Response,
+    rejected: Option<String>,
+    error: &mut Option<RejectedAuthorInput>,
+    field: &'static str,
+) -> bool {
     // DragValue removes its temporary string on focus loss before returning.
     // Restore a rejected parse so saving cannot silently capture the old number.
-    if let Some(text) = rejected {
+    // Slider parses while typing; unchanged Enter need not reparse. After blur
+    // its response can use the track ID, so retain the original numeric-editor ID.
+    let rejected = rejected
+        .map(|text| RejectedAuthorInput {
+            field,
+            widget_id: response.id,
+            text,
+        })
+        .or_else(|| {
+            if !response.changed()
+                && !response
+                    .ctx
+                    .input(|input| input.key_pressed(egui::Key::Escape))
+            {
+                error.as_ref().filter(|error| error.field == field).cloned()
+            } else {
+                None
+            }
+        });
+    if let Some(rejected) = rejected {
+        let id = rejected.widget_id;
         response
             .ctx
-            .data_mut(|data| data.insert_temp(response.id, text));
-        response.request_focus();
+            .data_mut(|data| data.insert_temp(id, rejected.text.clone()));
+        response.ctx.memory_mut(|memory| {
+            if !memory.has_focus(id) {
+                memory.request_focus(id);
+            }
+        });
+        *error = Some(rejected);
         return true;
     }
+    if response.changed() {
+        *error = None;
+    }
     response.has_focus()
 }
 
 fn activity_label(activity: TectonicActivity) -> &'static str {
     match activity {
         TectonicActivity::Quiet => "宁静",
         TectonicActivity::Moderate => "适中",
         TectonicActivity::Active => "活跃",
     }
 }
@@ -3242,20 +3431,363 @@ mod natural_app_tests {
         for text in ["1", "8"] {
             authoring_keyboard_frame(&mut app, &context, vec![egui::Event::Text(text.to_owned())]);
             assert_eq!(app.authoring.revision(), 0);
             assert_eq!(app.authoring.draft().tectonic_spec.plate_count, before);
         }
         authoring_keyboard_frame(&mut app, &context, vec![authoring_key(egui::Key::Enter)]);
         assert_eq!(app.authoring.draft().tectonic_spec.plate_count, 18);
         assert_eq!(app.authoring.revision(), 1);
     }
 
+    // Actual widgets are required: egui clamps/parses before the authoring transaction.
+    fn final_fix_percentage_app(continental: bool, value: f32) -> TemplateApp {
+        let mut inputs = super::AuthorInputs {
+            world_pipeline: WorldPipeline::LegacyFoundation,
+            ..Default::default()
+        };
+        inputs.relief_spec.target_land_fraction = if continental { 0.31 } else { value };
+        inputs.tectonic_spec.continental_crust_fraction = if continental { value } else { 0.31 };
+        TemplateApp {
+            authoring: super::AuthoringSession::new(inputs).unwrap(),
+            ..TemplateApp::default()
+        }
+    }
+
+    fn final_fix_focus_percentage(app: &mut TemplateApp, ctx: &egui::Context, continental: bool) {
+        let output = authoring_keyboard_frame(app, ctx, Vec::new());
+        let mut visible = Vec::new();
+        for shape in &output.shapes {
+            collect_text(&shape.shape, &mut visible);
+        }
+        if continental
+            && !visible
+                .iter()
+                .any(|text| text.ends_with('%') && text != "31%")
+        {
+            focus_authoring_number(app, ctx, "高级");
+        }
+        let output = authoring_keyboard_frame(app, ctx, Vec::new());
+        let mut text = Vec::new();
+        for shape in &output.shapes {
+            collect_text(&shape.shape, &mut text);
+        }
+        let percentage = text
+            .iter()
+            .find(|text| text.ends_with('%') && *text != "31%")
+            .expect("the editable percentage is rendered")
+            .clone();
+        focus_authoring_number(app, ctx, &percentage);
+    }
+
+    #[test]
+    fn final_fix_percentage_keyboard_accepts_display_units_and_escape_discards() {
+        for continental in [false, true] {
+            for input in ["40%", "40"] {
+                let mut app = final_fix_percentage_app(continental, 0.38);
+                let ctx = egui::Context::default();
+                let before = app.authoring.draft().clone();
+                let token = app.authoring.begin_build();
+                final_fix_focus_percentage(&mut app, &ctx, continental);
+                authoring_keyboard_frame(&mut app, &ctx, vec![egui::Event::Text(input.into())]);
+                assert_eq!(app.authoring.draft(), &before);
+                authoring_keyboard_frame(&mut app, &ctx, vec![authoring_key(egui::Key::Enter)]);
+                let value = if continental {
+                    app.authoring
+                        .draft()
+                        .tectonic_spec
+                        .continental_crust_fraction
+                } else {
+                    app.authoring.draft().relief_spec.target_land_fraction
+                };
+                assert_eq!(value, 0.40, "continental={continental} input={input}");
+                assert_eq!(app.authoring.revision(), 1);
+                assert!(!app.authoring.accepts(token));
+                let committed = app.authoring.draft().clone();
+                final_fix_focus_percentage(&mut app, &ctx, continental);
+                authoring_keyboard_frame(&mut app, &ctx, vec![egui::Event::Text("50%".into())]);
+                authoring_keyboard_frame(&mut app, &ctx, vec![authoring_key(egui::Key::Escape)]);
+                assert_eq!(app.authoring.draft(), &committed);
+                assert_eq!(app.authoring.revision(), 1);
+                assert!(app.authoring.undo());
+                assert_eq!(app.authoring.draft(), &before);
+                assert!(!app.authoring.undo());
+            }
+        }
+    }
+
+    #[test]
+    fn final_fix_percentage_untouched_enter_preserves_noninteger_recipe_bits() {
+        for continental in [false, true] {
+            let mut app = final_fix_percentage_app(continental, 0.38123456);
+            let ctx = egui::Context::default();
+            let before = app.authoring.draft().clone();
+            let identity = before.recipe_id().unwrap();
+            let token = app.authoring.begin_build();
+            final_fix_focus_percentage(&mut app, &ctx, continental);
+            authoring_keyboard_frame(&mut app, &ctx, vec![authoring_key(egui::Key::Enter)]);
+            assert_eq!(app.authoring.draft(), &before, "continental={continental}");
+            assert_eq!(app.authoring.draft().recipe_id().unwrap(), identity);
+            assert_eq!(app.authoring.revision(), 0);
+            assert!(app.authoring.accepts(token));
+            assert!(!app.authoring.undo());
+            assert!(
+                app.input_edit.is_none(),
+                "untouched Enter finishes without a pending edit"
+            );
+        }
+    }
+
+    #[test]
+    fn final_fix_plate_keyboard_rejects_outside_domain_before_clamp_or_round() {
+        for invalid in ["0", "999", "2.5"] {
+            for finish in [egui::Key::Enter, egui::Key::Tab] {
+                let mut app = TemplateApp::default();
+                let ctx = egui::Context::default();
+                let before = app.authoring.draft().clone();
+                let token = app.authoring.begin_build();
+                focus_authoring_number(
+                    &mut app,
+                    &ctx,
+                    &before.tectonic_spec.plate_count.to_string(),
+                );
+                authoring_keyboard_frame(&mut app, &ctx, vec![egui::Event::Text(invalid.into())]);
+                authoring_keyboard_frame(&mut app, &ctx, vec![authoring_key(finish)]);
+                assert_eq!(
+                    app.authoring.draft(),
+                    &before,
+                    "invalid={invalid} finish={finish:?}"
+                );
+                assert_eq!(app.authoring.revision(), 0);
+                assert!(app.authoring.accepts(token));
+                assert!(app.input_edit.is_some());
+                assert!(!app.authoring.undo());
+                let output = authoring_keyboard_frame(&mut app, &ctx, Vec::new());
+                let mut text = Vec::new();
+                for shape in &output.shapes {
+                    collect_text(&shape.shape, &mut text);
+                }
+                assert!(text.iter().any(|text| text == invalid), "rejected raw text stays visible: invalid={invalid} finish={finish:?} rendered={text:?}");
+                #[cfg(not(target_arch = "wasm32"))]
+                {
+                    app.dispatch_project_action(super::project_ui::ProjectAction::Save, &ctx);
+                    assert!(
+                        app.input_edit.is_some(),
+                        "saving refuses the pending invalid text"
+                    );
+                }
+                authoring_keyboard_frame(&mut app, &ctx, vec![authoring_key(egui::Key::Escape)]);
+                assert_eq!(app.authoring.draft(), &before);
+                assert!(app.input_edit.is_none());
+                focus_authoring_number(
+                    &mut app,
+                    &ctx,
+                    &before.tectonic_spec.plate_count.to_string(),
+                );
+                authoring_keyboard_frame(&mut app, &ctx, vec![egui::Event::Text("18".into())]);
+                authoring_keyboard_frame(&mut app, &ctx, vec![authoring_key(egui::Key::Enter)]);
+                assert_eq!(app.authoring.draft().tectonic_spec.plate_count, 18);
+                assert_eq!(app.authoring.revision(), 1);
+                assert!(!app.authoring.accepts(token));
+            }
+        }
+    }
+
+    #[test]
+    fn final_fix_percentage_keyboard_rejects_domain_errors_without_committing() {
+        for continental in [false, true] {
+            for invalid in ["0%", "999%", "NaN"] {
+                let mut app = final_fix_percentage_app(continental, 0.38);
+                let ctx = egui::Context::default();
+                let before = app.authoring.draft().clone();
+                let token = app.authoring.begin_build();
+                final_fix_focus_percentage(&mut app, &ctx, continental);
+                authoring_keyboard_frame(&mut app, &ctx, vec![egui::Event::Text(invalid.into())]);
+                authoring_keyboard_frame(&mut app, &ctx, vec![authoring_key(egui::Key::Enter)]);
+                assert_eq!(
+                    app.authoring.draft(),
+                    &before,
+                    "continental={continental} invalid={invalid}"
+                );
+                assert_eq!(app.authoring.revision(), 0);
+                assert!(app.authoring.accepts(token));
+                assert!(
+                    app.input_edit.is_some(),
+                    "continental={continental} invalid={invalid} focused={:?} error={:?}",
+                    ctx.memory(|m| m.focused()),
+                    app.author_input_error
+                );
+                authoring_keyboard_frame(&mut app, &ctx, vec![authoring_key(egui::Key::Escape)]);
+                assert!(app.input_edit.is_none());
+                assert_eq!(app.authoring.draft(), &before);
+            }
+        }
+    }
+
+    #[test]
+    fn final_fix_pending_status_renders_actual_surface_phase_without_observation() {
+        let mut app = TemplateApp::default();
+        let (_sender, receiver) = std::sync::mpsc::channel();
+        let (_, pending) = super::prepare_pending_world_build(
+            app.authoring.draft().world_pipeline,
+            MemoryStageCache::default(),
+            receiver,
+            (
+                app.authoring.begin_build(),
+                crate::engine::BuildCancellation::new(),
+            ),
+            false,
+        );
+        app.world_build = Some(pending);
+        assert!(app.build_observation.is_none());
+        let ctx = egui::Context::default();
+        let output = ctx.run(test_raw_input(), |ctx| {
+            egui::CentralPanel::default().show(ctx, |ui| {
+                app.show_pending_world_build_status(ui);
+            });
+        });
+        let mut text = Vec::new();
+        for shape in &output.shapes {
+            collect_text(&shape.shape, &mut text);
+        }
+        assert!(
+            text.iter().any(|text| text.contains("地表准备")),
+            "actual status text: {text:?}"
+        );
+    }
+
+    #[test]
+    fn final_fix_invalid_numbers_can_be_corrected_in_place() {
+        for field in ["plates", "land", "continental"] {
+            let continental = field == "continental";
+            let mut app = final_fix_percentage_app(continental, 0.38);
+            let ctx = egui::Context::default();
+            let token = app.authoring.begin_build();
+            if field == "plates" {
+                focus_authoring_number(&mut app, &ctx, "12");
+            } else {
+                final_fix_focus_percentage(&mut app, &ctx, continental);
+            }
+            authoring_keyboard_frame(&mut app, &ctx, vec![egui::Event::Text("0".into())]);
+            authoring_keyboard_frame(&mut app, &ctx, vec![authoring_key(egui::Key::Tab)]);
+            assert!(app.input_edit.is_some(), "field={field}");
+            assert_eq!(app.authoring.revision(), 0);
+            assert!(app.authoring.accepts(token));
+            #[cfg(not(target_arch = "wasm32"))]
+            {
+                app.dispatch_project_action(super::project_ui::ProjectAction::Save, &ctx);
+                assert!(app.input_edit.is_some());
+            }
+            authoring_keyboard_frame(&mut app, &ctx, Vec::new());
+            let replacement = if field == "plates" { "18" } else { "40%" };
+            let correction_output = authoring_keyboard_frame(
+                &mut app,
+                &ctx,
+                vec![
+                    egui::Event::Key {
+                        key: egui::Key::A,
+                        physical_key: None,
+                        pressed: true,
+                        repeat: false,
+                        modifiers: egui::Modifiers {
+                            ctrl: true,
+                            command: true,
+                            ..egui::Modifiers::NONE
+                        },
+                    },
+                    egui::Event::Text(replacement.into()),
+                ],
+            );
+            authoring_keyboard_frame(&mut app, &ctx, vec![authoring_key(egui::Key::Enter)]);
+            let mut correction_text = Vec::new();
+            for shape in &correction_output.shapes {
+                collect_text(&shape.shape, &mut correction_text);
+            }
+            assert!(
+                app.input_edit.is_none(),
+                "field={field} error={:?} focus={:?} input={:?} correction={correction_text:?}",
+                app.author_input_error,
+                ctx.memory(|m| m.focused()),
+                app.input_edit
+            );
+            assert!(app.author_input_error.is_none());
+            assert_eq!(app.authoring.revision(), 1);
+            assert!(!app.authoring.accepts(token));
+            match field {
+                "plates" => assert_eq!(app.authoring.draft().tectonic_spec.plate_count, 18),
+                "land" => assert_eq!(app.authoring.draft().relief_spec.target_land_fraction, 0.40),
+                _ => assert_eq!(
+                    app.authoring
+                        .draft()
+                        .tectonic_spec
+                        .continental_crust_fraction,
+                    0.40
+                ),
+            }
+            assert!(app.authoring.undo());
+            assert!(!app.authoring.undo());
+        }
+    }
+
+    // The cheapest actual worker proves phase updates with observation disabled;
+    // a renderer-only fixture cannot prove that production advances its state.
+    #[test]
+    fn final_fix_actual_worker_phase_is_request_owned_and_never_persisted() {
+        let mut inputs = super::AuthorInputs {
+            world_pipeline: WorldPipeline::LegacyFoundation,
+            ..Default::default()
+        };
+        inputs.spherical_space_spec.target_cell_count = 162;
+        let mut app = TemplateApp {
+            authoring: super::AuthoringSession::new(inputs).unwrap(),
+            ..TemplateApp::default()
+        };
+        app.request_spherical_world_build();
+        assert!(app.build_observation.is_none());
+        let pending = app.world_build.as_mut().unwrap();
+        pending.worker.take().unwrap().join().unwrap();
+        let old_phase = pending.phase.clone();
+        assert_eq!(
+            *old_phase.lock().unwrap(),
+            super::WorldBuildPhase::Presentation
+        );
+        assert!(pending.receiver.try_recv().unwrap().result.is_ok());
+        let ctx = egui::Context::default();
+        let output = ctx.run(test_raw_input(), |ctx| {
+            egui::CentralPanel::default().show(ctx, |ui| {
+                app.show_pending_world_build_status(ui);
+            });
+        });
+        let mut text = Vec::new();
+        for shape in &output.shapes {
+            collect_text(&shape.shape, &mut text);
+        }
+        assert!(text.iter().any(|text| text.contains("显示准备/发布")));
+        app.world_build = None;
+        let (_sender, receiver) = std::sync::mpsc::channel();
+        let (_, next) = super::prepare_pending_world_build(
+            WorldPipeline::LegacyFoundation,
+            MemoryStageCache::default(),
+            receiver,
+            (
+                app.authoring.begin_build(),
+                crate::engine::BuildCancellation::new(),
+            ),
+            false,
+        );
+        super::WorldBuildPhase::Graph.enter(Some(&old_phase));
+        assert_eq!(*next.phase.lock().unwrap(), super::WorldBuildPhase::Surface);
+        app.world_build = Some(next);
+        let wire = serde_json::to_value(&app).unwrap();
+        assert!(wire.get("world_build").is_none());
+        assert!(wire.get("phase").is_none());
+        assert!(wire.get("author_input_error").is_none());
+    }
     #[test]
     fn persisted_recipe_has_one_input_source_and_no_runtime_or_history() {
         let app = TemplateApp::default();
         let wire = serde_json::to_value(&app).unwrap();
         assert!(wire.get("inputs").is_some());
         assert!(wire.get("world_seed").is_none());
         assert!(wire.get("authoring").is_none());
         assert!(wire.get("history").is_none());
         assert!(wire.get("stage_cache").is_none());
         assert!(wire.get("spherical_presentation").is_none());
@@ -4941,20 +5473,21 @@ mod natural_app_tests {
         }
     }
 
     #[test]
     fn pending_world_build_keeps_the_last_valid_map_browsable_until_publication() {
         let mut app = create_cpu_published_app();
         let before = published_source(&app);
         let (sender, receiver) = std::sync::mpsc::channel();
         app.world_build = Some(PendingWorldBuild {
             token: app.authoring.begin_build(),
+            phase: Arc::new(std::sync::Mutex::new(super::WorldBuildPhase::Surface)),
             worker: None,
             receiver,
             cancellation: crate::engine::BuildCancellation::new(),
             started_at: std::time::Instant::now(),
             replacement: true,
             retained_stage_cache: None,
             completion: None,
         });
 
         let context = egui::Context::default();
diff --git a/src/app/project_ui.rs b/src/app/project_ui.rs
index 28fa53b..d2e7e54 100644
--- a/src/app/project_ui.rs
+++ b/src/app/project_ui.rs
@@ -124,20 +124,21 @@ impl TemplateApp {
     pub(super) fn project_is_dirty(&self) -> bool {
         self.authoring.is_dirty() || self.input_edit.is_some()
     }
 
     /// Applies a UI command before any completion may settle.
     pub(super) fn dispatch_project_action(&mut self, action: ProjectAction, ctx: &egui::Context) {
         match action {
             ProjectAction::Undo | ProjectAction::Redo => {
                 release_project_text_focus(ctx);
                 self.input_edit = None;
+                self.author_input_error = None;
                 let changed = match action {
                     ProjectAction::Undo => self.authoring.undo(),
                     _ => self.authoring.redo(),
                 };
                 if changed {
                     self.cancel_world_build();
                 }
                 self.project_ui.status = None;
             }
             ProjectAction::CancelBuild => self.cancel_world_build(),
@@ -201,20 +202,21 @@ impl TemplateApp {
                 self.cancel_world_build();
                 self.project_ui.exit_approved = true;
                 ctx.send_viewport_cmd(egui::ViewportCommand::Close);
             }
             _ => {}
         }
     }
 
     fn clear_project_publication(&mut self) {
         self.input_edit = None;
+        self.author_input_error = None;
         self.spherical_presentation
             .with_resource(|published| *published = None);
         self.published_world_token = None;
         self.amplified_detail = None;
         self.amplified_mesh = None;
         self.amplified_map_projected = None;
         self.river_polylines = None;
         self.river_radius_m = None;
         self.formation_surface = None;
         self.stage_cache = crate::engine::MemoryStageCache::new();
@@ -608,20 +610,21 @@ impl TemplateApp {
             self.project_ui.status = Some("确认期间参数已变化，请重新确认项目操作".into());
             return;
         }
         match (pending.kind, choice) {
             (ConfirmationKind::Unsaved(action), ConfirmationChoice::Save) => {
                 self.project_ui.after_save = Some((pending.epoch, action));
                 self.dispatch_project_action(ProjectAction::Save, ctx);
             }
             (ConfirmationKind::Unsaved(action), ConfirmationChoice::Discard) => {
                 self.input_edit = None;
+                self.author_input_error = None;
                 self.continue_project_action(action, ctx);
             }
             (
                 ConfirmationKind::Compatibility {
                     path,
                     mut loaded,
                     current,
                     compatibility,
                     timing,
                 },
diff --git a/src/app/spherical_presentation.rs b/src/app/spherical_presentation.rs
index d5af6f1..7563981 100644
--- a/src/app/spherical_presentation.rs
+++ b/src/app/spherical_presentation.rs
@@ -1,13 +1,14 @@
 //! Composition root for one complete source-bound spherical presentation.
 
 use super::build_metrics::{BuildObservation, BuildPhase};
+use super::WorldBuildPhase;
 use std::sync::Arc;
 
 use eframe::egui_wgpu::wgpu;
 use thiserror::Error;
 
 use super::field_document::{
     prepare_spherical_document_layers, reconcile_spherical_document_camera,
     update_spherical_document_layers, FieldDocument,
 };
 use super::spherical_formation_display::{
@@ -776,20 +777,21 @@ impl PublishedSphericalPresentation {
             tectonic,
             relief,
             geologic,
             cache,
             requested_view,
             requested_state,
             self.clock(),
             WorldCandidateLineage::Replacement(Arc::new(WorldReplaceBase::from_published(self))),
             &BuildCancellation::new(),
             None,
+            None,
         )
     }
 
     /// Returns an owned replacement authorization for off-thread candidate builds.
     ///
     /// The token binds a future candidate to this publication's world document.
     /// Display-only changes while the worker runs (fill, projection, camera,
     /// selection) do not invalidate it; [`Self::try_replace`] adopts the live
     /// display onto the finished world.
     pub fn replacement_token(&self) -> SphericalReplacementToken {
@@ -1653,60 +1655,64 @@ pub fn build_spherical_formation_candidate_for_view(
         tectonic,
         relief,
         geologic,
         cache,
         view_state,
         current_state,
         clock,
         WorldCandidateLineage::NoBase,
         &BuildCancellation::new(),
         None,
+        None,
     )
 }
 
 /// Formation-candidate builder that carries an explicit replacement lineage.
 #[allow(clippy::too_many_arguments)]
 fn build_spherical_formation_candidate_with_lineage(
     root_seed: RootSeed,
     quality_profile: NaturalQualityProfile,
     surface: &SphericalSurfaceSnapshot,
     formation: &WorldFormationSpec,
     tectonic: &TectonicSpec,
     relief: &ReliefSpec,
     geologic: &GeologicSpec,
     cache: &mut MemoryStageCache,
     view_state: SphericalPresentationViewState,
     current_state: &SphericalFieldDisplayState,
     clock: &DisplayRevisionClock,
     lineage: WorldCandidateLineage,
     cancellation: &BuildCancellation,
     observation: Option<&BuildObservation>,
+    phase: Option<&std::sync::Mutex<WorldBuildPhase>>,
 ) -> Result<SphericalPresentationCandidate, SphericalPresentationError> {
     let input_timing = observation.map(|o| o.span(BuildPhase::InputAndSurface, Some(1)));
     let external = build_spherical_formation_external_artifacts(
         root_seed,
         quality_profile,
         surface,
         formation,
         tectonic,
         relief,
         geologic,
     )?;
     drop(input_timing);
+    WorldBuildPhase::Graph.enter(phase);
     let graph_timing = observation.map(|o| o.span(BuildPhase::Graph, Some(1)));
     let outcome = BuildEngine::new(causal_natural_formation_graph()?).build_with_cancellation(
         root_seed,
         external,
         cache,
         cancellation,
     )?;
     drop(graph_timing);
+    WorldBuildPhase::Presentation.enter(phase);
     let _presentation_timing = observation.map(|o| o.span(BuildPhase::Presentation, Some(1)));
     let document = Arc::new(SphericalWorldFieldDocument::Formation(Arc::new(
         SphericalFormationFieldDocument::from_build_outcome(&outcome)?,
     )));
     assemble_spherical_candidate(
         document,
         outcome.report,
         view_state,
         current_state,
         clock,
@@ -1765,29 +1771,30 @@ pub struct SphericalReplacementToken {
 
 /// Executes one complete world-build request without touching the GPU.
 ///
 /// Safe to run on a worker thread; the caller keeps ownership of the stage
 /// cache and installs the returned candidate on the UI thread.
 pub fn run_spherical_world_build(
     request: SphericalWorldBuildRequest,
     cache: &mut MemoryStageCache,
     cancellation: &BuildCancellation,
 ) -> Result<SphericalPresentationCandidate, SphericalPresentationError> {
-    run_spherical_world_build_observed(request, cache, cancellation, None)
+    run_spherical_world_build_observed(request, cache, cancellation, None, None)
 }
 
 /// App-only timing at the existing causal boundaries; validation remains included.
 pub(super) fn run_spherical_world_build_observed(
     request: SphericalWorldBuildRequest,
     cache: &mut MemoryStageCache,
     cancellation: &BuildCancellation,
     observation: Option<&BuildObservation>,
+    phase: Option<&std::sync::Mutex<WorldBuildPhase>>,
 ) -> Result<SphericalPresentationCandidate, SphericalPresentationError> {
     let (lineage, clock) = match request.replacement {
         Some(token) => (token.lineage, token.clock),
         None => (
             WorldCandidateLineage::NoBase,
             DisplayRevisionClock::default(),
         ),
     };
     match request.target {
         SphericalWorldBuildTarget::Formation {
@@ -1801,35 +1808,38 @@ pub(super) fn run_spherical_world_build_observed(
             &request.tectonic,
             &request.relief,
             &request.geologic,
             cache,
             request.view_state,
             &request.requested_state,
             &clock,
             lineage,
             cancellation,
             observation,
+            phase,
         ),
         SphericalWorldBuildTarget::LegacyFoundation { space } => {
             let input_timing = observation.map(|o| o.span(BuildPhase::InputAndSurface, Some(1)));
             let external = build_spherical_external_artifacts(
                 &space,
                 &request.formation,
                 &request.tectonic,
                 &request.relief,
                 &request.geologic,
             )?;
             drop(input_timing);
+            WorldBuildPhase::Graph.enter(phase);
             let graph_timing = observation.map(|o| o.span(BuildPhase::Graph, Some(1)));
             let outcome = BuildEngine::new(spherical_natural_foundation_graph()?)
                 .build_with_cancellation(request.root_seed, external, cache, cancellation)?;
             drop(graph_timing);
+            WorldBuildPhase::Presentation.enter(phase);
             let _presentation_timing =
                 observation.map(|o| o.span(BuildPhase::Presentation, Some(1)));
             let document = Arc::new(SphericalWorldFieldDocument::NaturalFoundation(Arc::new(
                 SphericalNaturalFieldDocument::from_build_outcome(&outcome)?,
             )));
             assemble_spherical_candidate(
                 document,
                 outcome.report,
                 request.view_state,
                 &request.requested_state,
