use crate::resource::{CanvasStateResource, FieldDisplayResource, FieldViewerStateResource};
use crate::view::DisplayRevision;

use super::input::state_manager::InputStateManager;

const FIT_MARGIN_POINTS: f32 = 24.0;

pub struct Canvas {
    pub canvas_state_resource: CanvasStateResource,
    pub field_display_resource: FieldDisplayResource,
    pub field_viewer_state_resource: FieldViewerStateResource,
    pub input_state_manager: InputStateManager,
    fit_state: CanvasFitState,
}

impl Canvas {
    pub fn new(
        canvas_state_resource: CanvasStateResource,
        field_display_resource: FieldDisplayResource,
        field_viewer_state_resource: FieldViewerStateResource,
    ) -> Self {
        Self {
            canvas_state_resource: canvas_state_resource.clone(),
            field_display_resource,
            field_viewer_state_resource,
            input_state_manager: InputStateManager::new(canvas_state_resource),
            fit_state: CanvasFitState::default(),
        }
    }

    pub(super) fn fit_current_mesh_once(&mut self, screen_rect: egui::Rect) {
        let candidate = self.field_display_resource.read_resource(|display| {
            display
                .current()
                .map(|packet| (packet.revisions().mesh, packet.mesh().local_extent()))
        });
        let Some((mesh_revision, local_extent)) = candidate else {
            return;
        };
        if !self.fit_state.observe(mesh_revision) {
            return;
        }
        let Some(transform) = fit_transform(local_extent, screen_rect, FIT_MARGIN_POINTS) else {
            return;
        };
        self.canvas_state_resource
            .with_resource(|state| state.transform = transform);
    }
}

#[derive(Debug, Default)]
pub(super) struct CanvasFitState {
    fitted_mesh_revision: Option<DisplayRevision>,
}

impl CanvasFitState {
    pub(super) fn observe(&mut self, mesh_revision: DisplayRevision) -> bool {
        if self.fitted_mesh_revision == Some(mesh_revision) {
            return false;
        }
        self.fitted_mesh_revision = Some(mesh_revision);
        true
    }
}

pub(super) fn fit_transform(
    local_extent: [f32; 2],
    screen_rect: egui::Rect,
    margin: f32,
) -> Option<egui::emath::TSTransform> {
    if local_extent
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
        || !screen_rect.is_finite()
        || !margin.is_finite()
        || margin < 0.0
    {
        return None;
    }
    let available = screen_rect.size() - egui::vec2(margin * 2.0, margin * 2.0);
    if !available.x.is_finite()
        || !available.y.is_finite()
        || available.x <= 0.0
        || available.y <= 0.0
    {
        return None;
    }
    let scaling = (available.x / local_extent[0]).min(available.y / local_extent[1]);
    if !scaling.is_finite() || scaling <= 0.0 {
        return None;
    }
    let scaled_extent = egui::vec2(local_extent[0] * scaling, local_extent[1] * scaling);
    let translation = screen_rect.center().to_vec2() - scaled_extent * 0.5;
    Some(egui::emath::TSTransform {
        scaling,
        translation,
    })
}

#[cfg(test)]
mod tests {
    use super::{fit_transform, CanvasFitState};
    use crate::view::DisplayRevision;

    #[test]
    fn fit_transform_centers_geological_extent_inside_panel_margin() {
        let panel = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1000.0, 600.0));
        let transform = fit_transform([20_000_000.0, 10_000_000.0], panel, 24.0).unwrap();
        let mapped = transform.mul_rect(egui::Rect::from_min_max(
            egui::Pos2::ZERO,
            egui::pos2(20_000_000.0, 10_000_000.0),
        ));

        assert!(mapped.min.x >= panel.min.x + 24.0 - 0.01);
        assert!(mapped.min.y >= panel.min.y + 24.0 - 0.01);
        assert!(mapped.max.x <= panel.max.x - 24.0 + 0.01);
        assert!(mapped.max.y <= panel.max.y - 24.0 + 0.01);
        assert!((mapped.center() - panel.center()).length() < 0.01);
        assert!(transform.scaling < 0.001);
    }

    #[test]
    fn mesh_revision_refits_once_while_field_only_changes_do_not() {
        let mut state = CanvasFitState::default();
        let first_mesh = DisplayRevision::new(1).unwrap();
        assert!(state.observe(first_mesh));
        assert!(!state.observe(first_mesh));
        assert!(!state.observe(first_mesh));
        let second_mesh = DisplayRevision::new(5).unwrap();
        assert!(state.observe(second_mesh));
        assert!(!state.observe(second_mesh));
    }
}
