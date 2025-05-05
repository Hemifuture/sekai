//particle_shader.wgsl

struct Pos2 {
    x: f32,
    y: f32,
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
var<storage, read> points : array<Pos2>;

@group(0) @binding(1)
var<uniform> uniforms : CanvasUniforms; //x: time, y,z,w: 保留

struct VSOutput {
    @builtin(position) pos : vec4<f32>,
    @location(0) color : vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index : u32) -> VSOutput {
    // 确定当前顶点对应哪个点
    let point_id = vertex_index / 3;
    // 确定当前顶点是矩形中的哪个顶点(0-5)
    
    // 获取点的位置
    let point = points[point_id];
    let screen_pos = get_screen_pos(point, uniforms);
    
    var out : VSOutput;
    out.pos = get_triangle_pos(vertex_index, screen_pos, uniforms);
    out.color = vec4<f32>(0.9, 0.5, 0.5, 0.7);
    return out;
}

@fragment
fn fs_main(in : VSOutput) -> @location(0) vec4<f32> {
    //简单返回传入的颜色
    return in.color;
}

fn get_screen_pos(point: Pos2, uniforms: CanvasUniforms) -> vec2<f32> {
    // 应用平移和缩放，然后将[0,2]范围调整为[-1,1]
    let x = (point.x * uniforms.scale + uniforms.translation_x - uniforms.canvas_x) / uniforms.canvas_width * 2.0 - 1.0;
    let y = -((point.y * uniforms.scale + uniforms.translation_y - uniforms.canvas_y) / uniforms.canvas_height * 2.0 - 1.0);
    return vec2<f32>(x, y);
}

fn get_triangle_pos(vertex_index: u32, point: vec2<f32>, uniforms: CanvasUniforms) -> vec4<f32> {
    //let size = 0.006;
    let raw_size = uniforms.scale * 0.006;
    let size = clamp(raw_size, raw_size, 0.01);
    let vertex_in_rect = vertex_index % 3;
    
    // 矩形的6个顶点(两个三角形)相对位置
    var offset = vec2<f32>(0.0, 0.0);
    
    // 根据vertex_in_rect确定偏移量
    switch vertex_in_rect {
        case 0u: { // 第一个三角形 - 左下
            offset = vec2<f32>(0, size);
        }
        case 1u: { // 第一个三角形 - 右下
            offset = vec2<f32>(-0.866 * size, -0.5 * size);
        }
        case 2u: { // 第一个三角形 - 左上
            offset = vec2<f32>(0.866 * size, -0.5 * size);
        }
        default: {}
    }
    let aspect_ratio = uniforms.canvas_width / uniforms.canvas_height;
    let pos = vec4<f32>(point.x + offset.x, point.y + offset.y * aspect_ratio, 0.0, 1.0);
    return pos;
}

