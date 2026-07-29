struct GpuCellVertex {
    position: vec2<f32>,
    cell: u32,
    padding: u32,
}

struct FieldUniforms {
    canvas_x: f32,
    canvas_y: f32,
    canvas_width: f32,
    canvas_height: f32,
    translation_x: f32,
    translation_y: f32,
    scale: f32,
    canvas_padding1: f32,
    canvas_padding2: f32,
    canvas_padding3: f32,
    local_extent: vec2<f32>,
    display_min: f32,
    display_max: f32,
    field_kind: u32,
    palette_len: u32,
    diagnostics_enabled: u32,
    diagnostic_info_index: u32,
    diagnostic_warning_index: u32,
    diagnostic_error_index: u32,
    padding: vec4<u32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@group(0) @binding(0)
var<storage, read> vertices: array<GpuCellVertex>;

@group(0) @binding(1)
var<storage, read> field_values: array<u32>;

@group(0) @binding(2)
var<storage, read> diagnostics: array<u32>;

@group(0) @binding(3)
var<storage, read> palette: array<vec4<f32>>;

@group(0) @binding(4)
var<uniform> uniforms: FieldUniforms;

fn screen_position(local: vec2<f32>) -> vec2<f32> {
    let x = (
        local.x * uniforms.scale
        + uniforms.translation_x
        - uniforms.canvas_x
    ) / uniforms.canvas_width * 2.0 - 1.0;
    let y = -(
        (
            local.y * uniforms.scale
            + uniforms.translation_y
            - uniforms.canvas_y
        ) / uniforms.canvas_height * 2.0 - 1.0
    );
    return vec2<f32>(x, y);
}

fn sample_palette(t: f32) -> vec4<f32> {
    if uniforms.palette_len == 1u {
        return palette[0u];
    }
    let scaled = clamp(t, 0.0, 1.0) * f32(uniforms.palette_len - 1u);
    let lower = u32(floor(scaled));
    let upper = min(lower + 1u, uniforms.palette_len - 1u);
    return mix(palette[lower], palette[upper], scaled - f32(lower));
}

fn apply_diagnostic(base: vec4<f32>, severity: u32) -> vec4<f32> {
    if uniforms.diagnostics_enabled == 0u || severity == 0u {
        return base;
    }
    if severity == 1u {
        return palette[uniforms.diagnostic_info_index];
    }
    if severity == 2u {
        return palette[uniforms.diagnostic_warning_index];
    }
    return palette[uniforms.diagnostic_error_index];
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let vertex = vertices[vertex_index];
    let local_position = vertex.position * uniforms.local_extent;
    let raw = field_values[vertex.cell];

    var color: vec4<f32>;
    if uniforms.field_kind == 0u {
        let value = bitcast<f32>(raw);
        let width = uniforms.display_max - uniforms.display_min;
        var t = 0.5;
        if width > 0.0 {
            t = clamp((value - uniforms.display_min) / width, 0.0, 1.0);
        }
        color = sample_palette(t);
    } else {
        color = palette[raw % uniforms.palette_len];
    }
    color = apply_diagnostic(color, diagnostics[vertex.cell]);

    var output: VertexOutput;
    output.position = vec4<f32>(screen_position(local_position), 0.0, 1.0);
    output.color = color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
