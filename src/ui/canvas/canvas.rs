use crate::resource::{CanvasStateResource, MapSystemResource};

use super::input::state_manager::InputStateManager;

pub struct Canvas {
    pub canvas_state_resource: CanvasStateResource,
    pub map_system_resource: MapSystemResource,
    pub input_state_manager: InputStateManager,
}

impl Canvas {
    pub fn new(
        canvas_state_resource: CanvasStateResource,
        map_system_resource: MapSystemResource,
    ) -> Self {
        Self {
            canvas_state_resource: canvas_state_resource.clone(),
            map_system_resource,
            input_state_manager: InputStateManager::new(canvas_state_resource),
        }
    }
}
