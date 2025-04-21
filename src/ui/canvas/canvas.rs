use crate::resource::CanvasStateResource;

use super::input::state_manager::InputStateManager;

pub struct Canvas {
    pub canvas_state_resource: CanvasStateResource,
    pub input_state_manager: InputStateManager,
}

impl Canvas {
    pub fn new(canvas_state_resource: CanvasStateResource) -> Self {
        Self {
            canvas_state_resource: canvas_state_resource.clone(),
            input_state_manager: InputStateManager::new(canvas_state_resource),
        }
    }
}
