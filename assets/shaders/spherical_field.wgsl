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
    viewport_pixels: vec2<f32>,
    vector_phase: f32,
    globe_silhouette_clip: u32,
    fill_visible: u32,
    overlay_visible: u32,
    amplified_mode: u32,
    _padding: u32,
    // The camera-relative transform for rebased detail geometry (amplified
    // subdivision and rivers): identical scale columns, translation folded
    // with the rebase anchor in f64 on the CPU, so deep zooms stay crisp.
    detail_transform: mat4x4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

struct AmplifiedOutput {
    @builtin(position) position: vec4<f32>,
    // Flat interpolation reads the provoking vertex, so every subdivision
    // triangle renders as one solid patch like the cell view's units.
    @location(0) @interpolate(flat) color: vec4<f32>,
}

struct OverlayOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) along_arrow: f32,
    @location(2) @interpolate(flat) kind: u32,
    @location(3) @interpolate(linear) local_ndc: vec2<f32>,
}

@group(0) @binding(0)
var<storage, read> fill_values: array<u32>;

@group(0) @binding(1)
var<storage, read> diagnostic_severity: array<u32>;

@group(0) @binding(2)
var<storage, read> fill_palette: array<vec4<f32>>;

@group(0) @binding(3)
var<uniform> frame: SphericalFrameUniform;

// The amplified subdivision mesh carries pre-lit sRGB vertex colors; decode
// exactly the inverse of the CPU encoding so the sRGB target round-trips.
fn srgb_to_linear(encoded: vec3<f32>) -> vec3<f32> {
    let low = encoded / 12.92;
    let high = pow((encoded + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, encoded <= vec3<f32>(0.04045));
}

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
    var base = vec4<f32>(0.0);
    if frame.fill_visible != 0u {
        base = decode_fill_color(cell);
    }
    output.color = apply_diagnostic_overlay(base, cell);
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

@vertex
fn vs_map_amplified(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> AmplifiedOutput {
    var output: AmplifiedOutput;
    output.position = frame.detail_transform * vec4<f32>(position, 0.0, 1.0);
    output.color = color;
    return output;
}

@vertex
fn vs_globe_amplified(
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
) -> AmplifiedOutput {
    var output: AmplifiedOutput;
    output.position = frame.detail_transform * vec4<f32>(position, 1.0);
    output.color = color;
    return output;
}

@fragment
fn fs_amplified(input: AmplifiedOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(srgb_to_linear(input.color.rgb), input.color.a);
}

fn clipped_overlay(color: vec4<f32>, kind: u32) -> OverlayOutput {
    var output: OverlayOutput;
    output.position = vec4<f32>(0.0, 0.0, 2.0, 1.0);
    output.color = color;
    output.along_arrow = 0.0;
    output.kind = kind;
    output.local_ndc = vec2<f32>(0.0);
    return output;
}

fn quad_along(vertex: u32) -> f32 {
    if vertex == 0u || vertex == 1u || vertex == 3u {
        return 0.0;
    }
    return 1.0;
}

fn quad_side(vertex: u32) -> f32 {
    if vertex == 0u || vertex == 3u || vertex == 5u {
        return -1.0;
    }
    return 1.0;
}

fn expanded_overlay_vertex(
    start: vec4<f32>,
    end: vec4<f32>,
    width: f32,
    color: vec4<f32>,
    kind: u32,
    vertex: u32,
) -> OverlayOutput {
    if vertex >= 6u && kind == 0u {
        return clipped_overlay(color, kind);
    }
    let start_ndc = start.xy / start.w;
    let end_ndc = end.xy / end.w;
    let delta_pixels = (end_ndc - start_ndc) * frame.viewport_pixels * 0.5;
    let pixel_length = length(delta_pixels);
    if pixel_length <= 0.0001 {
        return clipped_overlay(color, kind);
    }
    let direction_pixels = delta_pixels / pixel_length;
    let perpendicular_pixels = vec2<f32>(-direction_pixels.y, direction_pixels.x);
    let perpendicular_ndc = perpendicular_pixels * width / frame.viewport_pixels;
    var output: OverlayOutput;
    output.color = color;
    output.kind = kind;
    if vertex < 6u {
        let along = quad_along(vertex);
        let center = mix(start_ndc, end_ndc, along);
        let ndc = center + perpendicular_ndc * quad_side(vertex);
        let clip = mix(start, end, along);
        output.position = vec4<f32>(ndc * clip.w, clip.z, clip.w);
        output.along_arrow = along;
        output.local_ndc = ndc;
        return output;
    }
    let head_back = direction_pixels * (width * 7.0) * 2.0 / frame.viewport_pixels;
    let head_side = perpendicular_pixels * (width * 3.0) * 2.0 / frame.viewport_pixels;
    var ndc = end_ndc;
    if vertex == 7u {
        ndc = end_ndc - head_back - head_side;
    } else if vertex == 8u {
        ndc = end_ndc - head_back + head_side;
    }
    output.position = vec4<f32>(ndc * end.w, end.z, end.w);
    output.along_arrow = 1.0;
    output.local_ndc = ndc;
    return output;
}

fn globe_overlay_from_clip(
    start_clip0: vec4<f32>,
    end_clip0: vec4<f32>,
    width: f32,
    color: vec4<f32>,
    kind: u32,
    vertex: u32,
) -> OverlayOutput {
    var start_clip = start_clip0;
    var end_clip = end_clip0;
    let start_horizon = start_clip.w * 0.5;
    let end_horizon = end_clip.w * 0.5;
    let start_front = start_clip.z >= start_horizon;
    let end_front = end_clip.z >= end_horizon;
    if kind == 1u && !start_front {
        return clipped_overlay(color, kind);
    }
    if !start_front && !end_front {
        return clipped_overlay(color, kind);
    }
    if start_front != end_front {
        let start_depth = start_clip.z - start_horizon;
        let end_depth = end_clip.z - end_horizon;
        let crossing = clamp(start_depth / (start_depth - end_depth), 0.0, 1.0);
        let clipped = mix(start_clip, end_clip, crossing);
        if !start_front {
            start_clip = clipped;
        } else {
            end_clip = clipped;
        }
    }
    return expanded_overlay_vertex(start_clip, end_clip, width, color, kind, vertex);
}

@vertex
fn vs_map_overlay(
    @location(0) start: vec2<f32>,
    @location(1) end: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) width: f32,
    @location(4) kind: u32,
    @builtin(vertex_index) vertex: u32,
) -> OverlayOutput {
    let start_clip = frame.transform * vec4<f32>(start, 0.0, 1.0);
    let end_clip = frame.transform * vec4<f32>(end, 0.0, 1.0);
    return expanded_overlay_vertex(start_clip, end_clip, width, color, kind, vertex);
}

@vertex
fn vs_globe_overlay(
    @location(0) start: vec3<f32>,
    @location(1) width: f32,
    @location(2) end_or_direction: vec3<f32>,
    @location(3) arrow_length: f32,
    @location(4) color: vec4<f32>,
    @location(5) kind: u32,
    @builtin(vertex_index) vertex: u32,
) -> OverlayOutput {
    var end = end_or_direction;
    if kind == 1u {
        end = start * cos(arrow_length) + end_or_direction * sin(arrow_length);
    }
    let start_clip = frame.transform * vec4<f32>(start, 1.0);
    let end_clip = frame.transform * vec4<f32>(end, 1.0);
    return globe_overlay_from_clip(start_clip, end_clip, width, color, kind, vertex);
}

// Rivers ride the overlay expansion but through the camera-relative
// detail transform: their instances are rebased with the amplified mesh
// so the polylines stay put on the terrain at deep zoom.
@vertex
fn vs_map_river(
    @location(0) start: vec2<f32>,
    @location(1) end: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) width: f32,
    @location(4) kind: u32,
    @builtin(vertex_index) vertex: u32,
) -> OverlayOutput {
    let start_clip = frame.detail_transform * vec4<f32>(start, 0.0, 1.0);
    let end_clip = frame.detail_transform * vec4<f32>(end, 0.0, 1.0);
    return expanded_overlay_vertex(start_clip, end_clip, width, color, kind, vertex);
}

@vertex
fn vs_globe_river(
    @location(0) start: vec3<f32>,
    @location(1) width: f32,
    @location(2) end_or_direction: vec3<f32>,
    @location(3) arrow_length: f32,
    @location(4) color: vec4<f32>,
    @location(5) kind: u32,
    @builtin(vertex_index) vertex: u32,
) -> OverlayOutput {
    let start_clip = frame.detail_transform * vec4<f32>(start, 1.0);
    let end_clip = frame.detail_transform * vec4<f32>(end_or_direction, 1.0);
    return globe_overlay_from_clip(start_clip, end_clip, width, color, kind, vertex);
}

// River polylines share the overlay expansion but ignore the overlay
// visibility switch and the vector animation: they belong to the terrain
// presentation itself.
@fragment
fn fs_river(input: OverlayOutput) -> @location(0) vec4<f32> {
    if frame.globe_silhouette_clip != 0u {
        let radius = vec2<f32>(
            length(vec3<f32>(frame.transform[0].x, frame.transform[1].x, frame.transform[2].x)),
            length(vec3<f32>(frame.transform[0].y, frame.transform[1].y, frame.transform[2].y)),
        );
        let normalized = input.local_ndc / radius;
        if dot(normalized, normalized) > 1.0 {
            discard;
        }
    }
    return input.color;
}

@fragment
fn fs_overlay(input: OverlayOutput) -> @location(0) vec4<f32> {
    if frame.overlay_visible == 0u {
        discard;
    }
    if frame.globe_silhouette_clip != 0u {
        let radius = vec2<f32>(
            length(vec3<f32>(frame.transform[0].x, frame.transform[1].x, frame.transform[2].x)),
            length(vec3<f32>(frame.transform[0].y, frame.transform[1].y, frame.transform[2].y)),
        );
        let normalized = input.local_ndc / radius;
        if dot(normalized, normalized) > 1.0 {
            discard;
        }
    }
    if input.kind == 0u {
        return input.color;
    }
    let moving = fract(input.along_arrow - frame.vector_phase);
    let highlight = 1.0 - smoothstep(0.0, 0.22, moving);
    return vec4<f32>(mix(input.color.rgb, vec3<f32>(1.0), highlight * 0.7), input.color.a);
}
