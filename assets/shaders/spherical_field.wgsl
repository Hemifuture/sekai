struct SphericalFrameUniform {
    transform: mat4x4<f32>,
    display_min: f32,
    display_max: f32,
    field_kind: u32,
    palette_len: u32,
    diagnostics_enabled: u32,
    diagnostic_info_index: u32,
    diagnostic_warning_index: u32,
    diagnostic_error_index: u32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@group(0) @binding(0)
var<storage, read> fill_values: array<u32>;

@group(0) @binding(1)
var<storage, read> diagnostic_severity: array<u32>;

@group(0) @binding(2)
var<storage, read> fill_palette: array<vec4<f32>>;

@group(0) @binding(3)
var<uniform> frame: SphericalFrameUniform;

fn sample_palette(t: f32) -> vec4<f32> {
    if frame.palette_len == 1u {
        return fill_palette[0u];
    }
    let scaled = clamp(t, 0.0, 1.0) * f32(frame.palette_len - 1u);
    let lower = u32(floor(scaled));
    let upper = min(lower + 1u, frame.palette_len - 1u);
    return mix(fill_palette[lower], fill_palette[upper], scaled - f32(lower));
}

fn decode_fill_color(cell: u32) -> vec4<f32> {
    let raw = fill_values[cell];
    if frame.field_kind == 0u {
        let value = bitcast<f32>(raw);
        let width = frame.display_max - frame.display_min;
        var t = 0.5;
        if width > 0.0 {
            t = clamp((value - frame.display_min) / width, 0.0, 1.0);
        }
        return sample_palette(t);
    }
    return fill_palette[raw % frame.palette_len];
}

fn apply_diagnostic_overlay(base: vec4<f32>, cell: u32) -> vec4<f32> {
    if frame.diagnostics_enabled == 0u {
        return base;
    }
    let severity = diagnostic_severity[cell];
    if severity == 0u {
        return base;
    }
    if severity == 1u {
        return fill_palette[frame.diagnostic_info_index];
    }
    if severity == 2u {
        return fill_palette[frame.diagnostic_warning_index];
    }
    return fill_palette[frame.diagnostic_error_index];
}

fn vertex_output(position: vec4<f32>, cell: u32) -> VertexOutput {
    var output: VertexOutput;
    output.position = frame.transform * position;
    output.color = apply_diagnostic_overlay(decode_fill_color(cell), cell);
    return output;
}

@vertex
fn vs_map(
    @location(0) position: vec2<f32>,
    @location(1) cell: u32,
) -> VertexOutput {
    return vertex_output(vec4<f32>(position, 0.0, 1.0), cell);
}

@vertex
fn vs_globe(
    @location(0) position: vec3<f32>,
    @location(1) cell: u32,
) -> VertexOutput {
    return vertex_output(vec4<f32>(position, 1.0), cell);
}

@fragment
fn fs_fill(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
