struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

// 定义传递给顶点着色器的所有点
@group(0) @binding(0)
var<storage, read> voronoi_edges: array<vec2<f32>>;

// 定义传递给顶点着色器的Uniform数据
@group(0) @binding(1)
var<uniform> uniforms: mat4x4<f32>;

// 着色器入口点
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    
    // 计算点坐标
    let point_index = vertex_index / 2u;
    let is_start = vertex_index % 2u == 0u;
    
    // 获取边的端点 - 数组索引安全性检查，防止越界
    var point: vec2<f32>;
    let max_index = arrayLength(&voronoi_edges) - 1u;
    
    // 确保索引在范围内
    let edge_index = min(point_index * 2u, max_index);
    let edge_index2 = min(edge_index + 1u, max_index);
    
    // 决定使用边的哪个点
    if (is_start) {
        point = voronoi_edges[edge_index];
    } else {
        point = voronoi_edges[edge_index2];
    }
    
    // 应用变换矩阵
    let transformed = uniforms * vec4<f32>(point.x, point.y, 0.0, 1.0);
    out.position = vec4<f32>(transformed.xy, 0.0, 1.0);
    
    // 设置颜色 - 使用柔和的浅蓝色
    out.color = vec4<f32>(0.3, 0.7, 0.9, 0.7);
    
    return out;
}

// 片元着色器，简单地输出顶点颜色
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
