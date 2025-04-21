//particle_shader.wgsl

struct Pos2 {
    x: f32,
    y: f32,
};

struct Triangle {
    points: array<Pos2, 3>,
};

struct CanvasUniforms {
    canvas_x: f32,
    canvas_y: f32,
    canvas_width: f32,
    canvas_height: f32,
    translation_x: f32,
    translation_y: f32,
    scale: f32,
    padding1: f32,
    padding2: f32,
    padding3: f32,
}

@group(0) @binding(0)
var<storage, read> triangles : array<Triangle>;

@group(0) @binding(1)
var<uniform> uniforms : CanvasUniforms; //x: time, y,z,w: 保留

struct VSOutput {
    @builtin(position) pos : vec4<f32>,
    @location(0) color : vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index : u32) -> VSOutput {
    // 确定当前顶点对应哪个三角形
    let triangle_id = vertex_index / 6u;
    // 确定当前是三角形的哪条边（每个三角形有3条边，每条边2个顶点）
    let edge_id = (vertex_index % 6u) / 2u;
    // 确定是边的起点还是终点（0是起点，1是终点）
    let is_end = vertex_index % 2u;
    
    // 获取三角形
    let triangle = triangles[triangle_id];
    
    // 计算当前点索引和下一个点索引（形成边）
    let current_point_idx = edge_id;
    let next_point_idx = (edge_id + 1u) % 3u;
    
    // 选择正确的点坐标（使用if语句替代select函数）
    var point_pos = Pos2(0.0, 0.0);
    if is_end == 0u {
        point_pos = triangle.points[current_point_idx];
    } else {
        point_pos = triangle.points[next_point_idx];
    };
    
    // 转换到屏幕坐标
    let screen_pos = vec4<f32>(get_screen_pos(point_pos, uniforms), 0.0, 1.0);
    
    var out : VSOutput;
    out.pos = screen_pos;
    out.color = vec4<f32>(0.0, 1.0, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in : VSOutput) -> @location(0) vec4<f32> {
    // 简单返回线条颜色
    return in.color;
}

fn get_screen_pos(point: Pos2, uniforms: CanvasUniforms) -> vec2<f32> {
    // 应用平移和缩放，然后将[0,2]范围调整为[-1,1]
    let x = (point.x * uniforms.scale + uniforms.translation_x - uniforms.canvas_x) / uniforms.canvas_width * 2.0 - 1.0;
    let y = -((point.y * uniforms.scale + uniforms.translation_y - uniforms.canvas_y) / uniforms.canvas_height * 2.0 - 1.0);
    return vec2<f32>(x, y);
}

fn get_triangle_pos(vertex_index: u32, point: vec2<f32>, uniforms: CanvasUniforms) -> vec4<f32> {
    let size = 0.01;
    let vertex_in_rect = vertex_index % 6;
    
    // 矩形的6个顶点(两个三角形)相对位置
    var offset = vec2<f32>(0.0, 0.0);
    
    // 根据vertex_in_rect确定偏移量
    switch vertex_in_rect {
        case 0u: { // 第一个三角形 - 左下
            offset = vec2<f32>(-size, -size);
        }
        case 1u: { // 第一个三角形 - 右下
            offset = vec2<f32>(size, -size);
        }
        case 2u: { // 第一个三角形 - 左上
            offset = vec2<f32>(-size, size);
        }
        case 3u: { // 第二个三角形 - 左上
            offset = vec2<f32>(-size, size);
        }
        case 4u: { // 第二个三角形 - 右下
            offset = vec2<f32>(size, -size);
        }
        case 5u: { // 第二个三角形 - 右上
            offset = vec2<f32>(size, size);
        }
        default: {}
    }
    let aspect_ratio = uniforms.canvas_width / uniforms.canvas_height;
    let pos = vec4<f32>(point.x + offset.x, point.y + offset.y * aspect_ratio, 0.0, 1.0);
    return pos;
}

