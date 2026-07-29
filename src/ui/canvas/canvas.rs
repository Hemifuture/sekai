use crate::resource::{
    CanvasStateResource, FieldDisplayResource, FieldViewerStateResource, MapSystemResource,
};

use super::input::state_manager::InputStateManager;

pub struct Canvas {
    pub canvas_state_resource: CanvasStateResource,
    pub map_system_resource: MapSystemResource,
    pub field_display_resource: FieldDisplayResource,
    pub field_viewer_state_resource: FieldViewerStateResource,
    pub input_state_manager: InputStateManager,
}

impl Canvas {
    pub fn new(
        canvas_state_resource: CanvasStateResource,
        map_system_resource: MapSystemResource,
        field_display_resource: FieldDisplayResource,
        field_viewer_state_resource: FieldViewerStateResource,
    ) -> Self {
        Self {
            canvas_state_resource: canvas_state_resource.clone(),
            map_system_resource,
            field_display_resource,
            field_viewer_state_resource,
            input_state_manager: InputStateManager::new(canvas_state_resource),
        }
    }
}
