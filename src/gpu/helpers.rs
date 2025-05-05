use egui::{Pos2, Rect};

use crate::resource::CanvasStateResource;

use super::canvas_uniform::CanvasUniforms;

/// 获取可见的边索引
///
/// 每2个索引为一条边
pub fn get_visible_indices(
    points: &Vec<Pos2>,
    uniforms: CanvasUniforms,
    indices: Vec<usize>,
    canvas_state_resource: CanvasStateResource,
) -> Vec<usize> {
    let mut visible_indices = Vec::new();

    let view_rect = canvas_state_resource.read_resource(|canvas_state| {
        canvas_state.to_canvas_rect(egui::Rect::from_min_max(
            egui::Pos2::new(uniforms.canvas_x, uniforms.canvas_y),
            egui::Pos2::new(
                uniforms.canvas_x + uniforms.canvas_width,
                uniforms.canvas_y + uniforms.canvas_height,
            ),
        ))
    });
    println!(
        "view_rect: {:?}, canvas_transform: {:?}",
        view_rect,
        canvas_state_resource.read_resource(|cs| cs.transform)
    );

    for chunk in indices.chunks(2) {
        if chunk.len() == 2 {
            let p1 = points[chunk[0] as usize];
            let p2 = points[chunk[1] as usize];

            // 首先检查端点是否在视口内
            if view_rect.contains(p1) || view_rect.contains(p2) {
                visible_indices.extend_from_slice(chunk);
                continue;
            }

            // 快速剔除：检查线段是否完全在视口的一侧
            if (p1.x < view_rect.min.x && p2.x < view_rect.min.x) || // 完全在左侧
                   (p1.x > view_rect.max.x && p2.x > view_rect.max.x) || // 完全在右侧
                   (p1.y < view_rect.min.y && p2.y < view_rect.min.y) || // 完全在上方
                   (p1.y > view_rect.max.y && p2.y > view_rect.max.y)
            {
                // 完全在下方
                continue; // 线段完全在视口外，跳过
            }

            // 线段不完全在视口外的同一侧，需要进一步检查相交
            if line_intersects_rect(p1, p2, view_rect) {
                visible_indices.extend_from_slice(chunk);
            }
        }
    }

    println!("visible edges count: {}", visible_indices.chunks(2).count());

    visible_indices
}

/// 优化的线段与矩形相交测试
fn line_intersects_rect(p1: Pos2, p2: Pos2, rect: Rect) -> bool {
    // Cohen-Sutherland算法的区域码
    fn compute_code(p: Pos2, rect: Rect) -> u8 {
        let mut code = 0;
        if p.x < rect.min.x {
            code |= 1;
        }
        // 左
        else if p.x > rect.max.x {
            code |= 2;
        } // 右
        if p.y < rect.min.y {
            code |= 4;
        }
        // 上
        else if p.y > rect.max.y {
            code |= 8;
        } // 下
        code
    }

    let code1 = compute_code(p1, rect);
    let code2 = compute_code(p2, rect);

    // 快速接受：两点都在矩形内
    if code1 == 0 && code2 == 0 {
        return true;
    }

    // 快速拒绝：两点位于矩形某一边界的外侧
    if (code1 & code2) != 0 {
        return false;
    }

    // 如果代码走到这里，说明线段可能与矩形相交
    // 使用参数方程法检查与矩形四条边的交点

    // 线段参数方程: p = p1 + t * (p2 - p1), t ∈ [0,1]
    // 计算与水平边界的交点
    let dx = p2.x - p1.x;
    let dy = p2.y - p1.y;

    // 避免除以零
    if dy.abs() > 1e-6 {
        // 与上边界y=rect.min.y相交
        let t_top = (rect.min.y - p1.y) / dy;
        if t_top >= 0.0 && t_top <= 1.0 {
            let x_intersect = p1.x + t_top * dx;
            if x_intersect >= rect.min.x && x_intersect <= rect.max.x {
                return true;
            }
        }

        // 与下边界y=rect.max.y相交
        let t_bottom = (rect.max.y - p1.y) / dy;
        if t_bottom >= 0.0 && t_bottom <= 1.0 {
            let x_intersect = p1.x + t_bottom * dx;
            if x_intersect >= rect.min.x && x_intersect <= rect.max.x {
                return true;
            }
        }
    }

    // 避免除以零
    if dx.abs() > 1e-6 {
        // 与左边界x=rect.min.x相交
        let t_left = (rect.min.x - p1.x) / dx;
        if t_left >= 0.0 && t_left <= 1.0 {
            let y_intersect = p1.y + t_left * dy;
            if y_intersect >= rect.min.y && y_intersect <= rect.max.y {
                return true;
            }
        }

        // 与右边界x=rect.max.x相交
        let t_right = (rect.max.x - p1.x) / dx;
        if t_right >= 0.0 && t_right <= 1.0 {
            let y_intersect = p1.y + t_right * dy;
            if y_intersect >= rect.min.y && y_intersect <= rect.max.y {
                return true;
            }
        }
    }

    false
}
