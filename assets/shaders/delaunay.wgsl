//delaunay.wgsl
// 这个着色器负责绘制Delaunay三角剖分的线框
// 使用LineList拓扑和索引缓冲区，直接绘制三角形边

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
var<storage, read> vertices : array<Pos2>;

@group(0) @binding(1)
var<uniform> uniforms : CanvasUniforms;

struct VSOutput {
    @builtin(position) pos : vec4<f32>,
    @location(0) color : vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index : u32) -> VSOutput {
    // 直接使用索引缓冲区中的索引获取顶点数据
    let point_pos = vertices[vertex_index];
    
    // 转换到屏幕坐标
    let screen_pos = vec4<f32>(get_screen_pos(point_pos, uniforms), 0.0, 1.0);
    
    var out : VSOutput;
    out.pos = screen_pos;
    out.color = vec4<f32>(0.6, 0.6, 0.6, 1.0);
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

// 以下函数不再使用，但保留作为参考
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

