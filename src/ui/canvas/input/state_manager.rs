// 在 src/input/state_manager.rs 中

use std::collections::HashMap;

use crate::resource::CanvasStateResource;

use egui::*;

use super::button_state::ButtonState;

/// 存储输入处理所需的上下文数据
#[derive(Debug)]
pub struct InputContext {
    /// 画布资源
    pub canvas_state_resource: CanvasStateResource,

    /// 当前鼠标位置（屏幕坐标）
    pub current_mouse_pos: Pos2,

    /// 前一帧的鼠标位置
    pub prev_mouse_pos: Pos2,

    /// 当前按键修饰符
    pub modifiers: Modifiers,

    /// 当前按下的鼠标按钮
    pub pressed_buttons: ButtonState,

    /// 当前按下的键
    pub pressed_keys: HashMap<Key, bool>,

    /// 从上一帧到当前帧的时间（秒）
    pub delta_time: f32,
}

impl InputContext {
    pub fn new(canvas_state_resource: CanvasStateResource) -> Self {
        Self {
            canvas_state_resource,
            current_mouse_pos: Pos2::ZERO,
            prev_mouse_pos: Pos2::ZERO,
            modifiers: Modifiers::NONE,
            pressed_buttons: ButtonState::new(),
            pressed_keys: HashMap::new(),
            delta_time: 0.0,
        }
    }

    /// 更新上下文中的每帧数据
    pub fn update(&mut self, ui: &mut egui::Ui) {
        ui.input(|i| {
            self.prev_mouse_pos = self.current_mouse_pos;
            self.current_mouse_pos = i.pointer.hover_pos().unwrap_or(self.current_mouse_pos);
            self.modifiers = i.modifiers;
            self.delta_time = i.stable_dt;

            // 更新按键状态
            for key in Key::ALL {
                self.pressed_keys.insert(*key, i.key_down(*key));
            }

            // 更新按钮状态
            self.pressed_buttons.set(
                PointerButton::Primary,
                i.pointer.button_down(PointerButton::Primary),
            );
            self.pressed_buttons.set(
                PointerButton::Secondary,
                i.pointer.button_down(PointerButton::Secondary),
            );
            self.pressed_buttons.set(
                PointerButton::Middle,
                i.pointer.button_down(PointerButton::Middle),
            );
            self.pressed_buttons.set(
                PointerButton::Extra1,
                i.pointer.button_down(PointerButton::Extra1),
            );
            self.pressed_buttons.set(
                PointerButton::Extra2,
                i.pointer.button_down(PointerButton::Extra2),
            );
        });
    }

    /// 将屏幕坐标转换为画布坐标
    pub fn screen_to_canvas(&self, screen_pos: Pos2) -> Pos2 {
        self.canvas_state_resource
            .read_resource(|canvas_state| canvas_state.to_canvas(screen_pos))
    }

    /// 将画布坐标转换为屏幕坐标
    pub fn canvas_to_screen(&self, canvas_pos: Pos2) -> Pos2 {
        self.canvas_state_resource
            .read_resource(|canvas_state| canvas_state.to_screen(canvas_pos))
    }
}

/// 平移和缩放状态
#[derive(Debug, Clone, PartialEq)]
pub enum PanZoomState {
    /// 空闲状态 - 系统等待新的输入
    Idle,

    /// 平移状态 - 用户正在平移画布
    Panning {
        last_cursor_pos: Pos2,
        dragging: bool,
    },
}

/// 输入状态管理器，仅处理画布平移和缩放
#[derive(Debug)]
pub struct InputStateManager {
    /// 当前输入状态
    pub current_state: PanZoomState,

    /// 输入上下文
    pub context: InputContext,
}

impl InputStateManager {
    pub fn new(canvas_state_resource: CanvasStateResource) -> Self {
        Self {
            current_state: PanZoomState::Idle,
            context: InputContext::new(canvas_state_resource),
        }
    }

    /// 转换到新状态
    pub fn transition_to(&mut self, new_state: PanZoomState) {
        // 可以在这里添加状态转换的日志或验证
        println!(
            "Input state transition: {:?} -> {:?}",
            self.current_state, new_state
        );
        self.current_state = new_state;
    }

    /// 每帧更新输入状态
    pub fn update(&mut self, ui: &mut egui::Ui) {
        // 更新上下文
        self.context.update(ui);

        // 首先处理一次性事件，这些可能导致状态转换
        self.handle_one_shot_events(ui);

        // 然后处理持续性事件
        self.handle_continuous_events(ui);

        // 处理状态特定的每帧逻辑
        self.handle_state_specific_updates(ui);
    }

    /// 处理可能触发状态转换的一次性事件
    fn handle_one_shot_events(&mut self, ui: &mut egui::Ui) {
        // 检查键盘按键
        if ui.input(|i| i.key_pressed(Key::Space)) {
            self.handle_space_key_press();
        }
        if ui.input(|i| i.key_released(Key::Space)) {
            self.handle_space_key_release();
        }

        // 检查鼠标点击
        if ui.input(|i| i.pointer.button_pressed(PointerButton::Primary)) {
            self.handle_primary_button_press(ui);
        }

        // 检查鼠标释放
        if ui.input(|i| i.pointer.button_released(PointerButton::Primary)) {
            self.handle_primary_button_release();
        }
    }

    /// 处理持续性事件
    fn handle_continuous_events(&mut self, ui: &mut egui::Ui) {
        // 处理鼠标移动
        if matches!(
            self.current_state,
            PanZoomState::Panning { dragging: true, .. }
        ) {
            let delta = ui.input(|i| i.pointer.delta());
            if delta != Vec2::ZERO {
                self.handle_mouse_motion(delta);
            }
        }

        // 处理滚动
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
        if scroll_delta != Vec2::ZERO {
            self.handle_scroll(scroll_delta);
        }

        // 处理缩放
        let zoom_delta = ui.input(|i| i.zoom_delta());
        if zoom_delta != 1.0 {
            self.handle_zoom(zoom_delta);
        }
    }

    /// 处理状态特定的更新逻辑
    fn handle_state_specific_updates(&mut self, ui: &mut egui::Ui) {
        match &self.current_state {
            PanZoomState::Idle => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
            }
            PanZoomState::Panning { .. } => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }
    }

    fn handle_space_key_press(&mut self) {
        if matches!(self.current_state, PanZoomState::Idle) {
            self.transition_to(PanZoomState::Panning {
                last_cursor_pos: self.context.current_mouse_pos,
                dragging: false,
            });
        }
    }

    fn handle_space_key_release(&mut self) {
        if matches!(
            self.current_state,
            PanZoomState::Panning {
                last_cursor_pos: _,
                dragging: _
            }
        ) {
            self.transition_to(PanZoomState::Idle);
        }
    }

    fn handle_primary_button_press(&mut self, ui: &mut egui::Ui) {
        let space_pressed = ui.input(|i| i.key_down(Key::Space));

        if space_pressed
            && matches!(
                self.current_state,
                PanZoomState::Panning {
                    dragging: false,
                    ..
                }
            )
        {
            // 按住空格键并点击鼠标，开始拖动
            self.transition_to(PanZoomState::Panning {
                last_cursor_pos: self.context.current_mouse_pos,
                dragging: true,
            });
        }
    }

    fn handle_primary_button_release(&mut self) {
        if matches!(
            self.current_state,
            PanZoomState::Panning { dragging: true, .. }
        ) {
            // 释放鼠标按键，停止拖动但保持平移状态
            self.transition_to(PanZoomState::Panning {
                last_cursor_pos: self.context.current_mouse_pos,
                dragging: false,
            });
        }
    }

    fn handle_mouse_motion(&mut self, delta: Vec2) {
        if matches!(
            self.current_state,
            PanZoomState::Panning { dragging: true, .. }
        ) {
            // 更新平移
            self.context.canvas_state_resource.with_resource(|state| {
                state.transform.translation += delta;
            });
        }
    }

    fn handle_scroll(&mut self, delta: Vec2) {
        // 平移画布
        self.context.canvas_state_resource.with_resource(|state| {
            state.transform.translation += delta;
        });
    }

    fn handle_zoom(&mut self, delta: f32) {
        // 缩放画布
        let mouse_pos = self.context.current_mouse_pos;

        self.context.canvas_state_resource.with_resource(|state| {
            let scaling = state.transform.scaling;
            if scaling <= 0.1 && delta < 1.0 || scaling >= 100.0 && delta > 1.0 {
                return;
            }
            let pointer_in_layer = state.transform.inverse() * mouse_pos;

            // 缩放，保持鼠标下方的点不变
            state.transform = state.transform
                * egui::emath::TSTransform::from_translation(pointer_in_layer.to_vec2())
                * egui::emath::TSTransform::from_scaling(delta)
                * egui::emath::TSTransform::from_translation(-pointer_in_layer.to_vec2());

            // 最终scaling截断
            state.transform.scaling = state.transform.scaling.clamp(0.1, 100.0);
        });
    }
}
