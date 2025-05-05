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

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

// 定义传递给顶点着色器的所有顶点
@group(0) @binding(0)
var<storage, read> voronoi_vertices: array<Pos2>;

// 定义顶点索引
@group(0) @binding(1)
var<storage, read> voronoi_indices: array<u32>;

// 定义传递给顶点着色器的Uniform数据
@group(0) @binding(2)
var<uniform> uniforms: CanvasUniforms;

// 着色器入口点
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    
    // 通过索引获取顶点
    let index = voronoi_indices[vertex_index];
    
    // 确保索引在范围内
    let max_index = arrayLength(&voronoi_vertices) - 1u;
    let safe_index = min(index, max_index);
    
    // 获取顶点位置
    let point = voronoi_vertices[safe_index];
    
    // 应用变换矩阵
    let transformed = get_screen_pos(point, uniforms);
    out.position = vec4<f32>(transformed, 0.0, 1.0);
    
    // 设置颜色 - 使用柔和的浅蓝色
    out.color = vec4<f32>(0.3, 0.7, 0.9, 1.0);
    
    return out;
}

// 片元着色器，简单地输出顶点颜色
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}

// 与Delaunay一致的坐标变换函数
fn get_screen_pos(point: Pos2, uniforms: CanvasUniforms) -> vec2<f32> {
    // 应用平移和缩放，然后将坐标范围调整为[-1,1]
    let x = (point.x * uniforms.scale + uniforms.translation_x - uniforms.canvas_x) / uniforms.canvas_width * 2.0 - 1.0;
    let y = -((point.y * uniforms.scale + uniforms.translation_y - uniforms.canvas_y) / uniforms.canvas_height * 2.0 - 1.0);
    return vec2<f32>(x, y);
}