// Height map shader for rendering Voronoi cells with height-based colors
// This shader renders filled polygons (Voronoi cells) with colors based on terrain height

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

// Vertex positions for triangulated Voronoi cells
@group(0) @binding(0)
var<storage, read> vertices: array<Pos2>;

// Per-vertex colors (interpolated from height map)
@group(0) @binding(1)
var<storage, read> colors: array<vec4<f32>>;

// Canvas transformation uniforms
@group(0) @binding(2)
var<uniform> uniforms: CanvasUniforms;

// Vertex shader - transforms vertices and passes colors through
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;

    // Safely access vertex data
    let max_index = arrayLength(&vertices) - 1u;
    let safe_index = min(vertex_index, max_index);

    // Get vertex position
    let point = vertices[safe_index];

    // Transform to screen space
    let transformed = get_screen_pos(point, uniforms);
    out.position = vec4<f32>(transformed, 0.0, 1.0);

    // Pass through vertex color
    out.color = colors[safe_index];

    return out;
}

// Fragment shader - outputs interpolated color
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}

// Transform canvas coordinates to NDC (Normalized Device Coordinates)
fn get_screen_pos(point: Pos2, uniforms: CanvasUniforms) -> vec2<f32> {
    // Apply scale and translation, then map to [-1, 1] range
    let x = (point.x * uniforms.scale + uniforms.translation_x - uniforms.canvas_x) / uniforms.canvas_width * 2.0 - 1.0;
    let y = -((point.y * uniforms.scale + uniforms.translation_y - uniforms.canvas_y) / uniforms.canvas_height * 2.0 - 1.0);
    return vec2<f32>(x, y);
}
