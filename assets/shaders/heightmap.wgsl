// 高度图渲染着色器 - 渲染填充的 Voronoi 单元格

struct Pos2 {
    x: f32,
    y: f32,
}

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
}

@group(0) @binding(0)
var<storage, read> vertices: array<Pos2>;

@group(0) @binding(1)
var<storage, read> colors: array<vec4<f32>>;

@group(0) @binding(2)
var<uniform> uniforms: CanvasUniforms;

// 将地图坐标转换为屏幕坐标
fn get_screen_pos(point: Pos2, uniforms: CanvasUniforms) -> vec2<f32> {
    let x = (point.x * uniforms.scale + uniforms.translation_x - uniforms.canvas_x)
            / uniforms.canvas_width * 2.0 - 1.0;
    let y = -((point.y * uniforms.scale + uniforms.translation_y - uniforms.canvas_y)
            / uniforms.canvas_height * 2.0 - 1.0);
    return vec2<f32>(x, y);
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;

    let vertex = vertices[vertex_index];
    let screen_pos = get_screen_pos(vertex, uniforms);

    out.position = vec4<f32>(screen_pos, 0.0, 1.0);
    out.color = colors[vertex_index];

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
