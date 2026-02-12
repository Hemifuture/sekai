//! Delaunay 三角剖分核心算法
//!
//! 本模块提供高效的 Delaunay 三角剖分实现，基于 `delaunator` 库。
//!
//! # Delaunay 三角剖分
//! Delaunay 三角剖分是一种将平面点集划分为三角形的方法，满足以下性质：
//! - 任意三角形的外接圆内不包含其他点
//! - 最大化最小角（避免狭长三角形）
//!
//! # 用途
//! - 作为 Voronoi 图的对偶图
//! - 地形高度插值
//! - 邻接关系计算
//!
//! # 性能
//! - 使用 `delaunator` 库，时间复杂度 O(n log n)
//! - 使用 `rayon` 并行预处理
//! - 10000 点约需 10-50ms
//!
//! # 索引类型
//! 使用 `u32` 作为索引类型，支持最多 40 亿个点，同时：
//! - 内存占用比 `usize` 减少 50%（64位系统）
//! - GPU 索引缓冲区原生支持

#[cfg(debug_assertions)]
use crate::delaunay::utils::calculate_convex_hull_indices;
use egui::Pos2;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use std::sync::Once;

static INIT_LOGGER: Once = Once::new();

// ============================================================================
// 公开 API
// ============================================================================

/// 执行 Delaunay 三角剖分
///
/// 将输入点集划分为不重叠的三角形网格，满足 Delaunay 性质。
///
/// # 参数
/// - `points`: 输入点坐标列表
///
/// # 返回值
/// 三角形索引列表（`Vec<u32>`），每3个连续索引构成一个三角形。
/// 索引指向输入 `points` 数组。
///
/// # 索引类型
/// 使用 `u32` 而非 `usize`，原因：
/// - 内存占用减少 50%（64位系统上 8 bytes → 4 bytes）
/// - GPU 索引缓冲区原生使用 u32
/// - 支持最多 40 亿个点，远超实际需求
///
/// # 算法流程
/// 1. 预处理：去除重复点（使用整数量化）
/// 2. 调用 `delaunator` 库执行三角剖分
/// 3. 将结果索引映射回原始点数组
///
/// # 边界情况
/// - 少于3个点：返回空列表
/// - 所有点共线：返回空列表
/// - 重复点：自动去重
///
/// # 示例
/// ```ignore
/// let points = vec![
///     Pos2::new(0.0, 0.0),
///     Pos2::new(1.0, 0.0),
///     Pos2::new(0.5, 1.0),
///     Pos2::new(1.0, 1.0),
/// ];
///
/// let indices = triangulate(&points);
/// // indices 可能是 [0, 1, 2, 1, 3, 2] 表示两个三角形
/// // 类型为 Vec<u32>
/// ```
pub fn triangulate(points: &[Pos2]) -> Vec<u32> {
    #[cfg(debug_assertions)]
    let start_time = std::time::Instant::now();

    // 首次调用时打印日志
    INIT_LOGGER.call_once(|| {
        println!("三角剖分开始，处理 {} 个点", points.len());
    });

    // 至少需要3个点才能形成三角形
    if points.len() < 3 {
        return Vec::new();
    }

    // Step 1: 预处理 - 并行去重
    let (unique_points, original_indices) = preprocess_points(points);

    #[cfg(debug_assertions)]
    let preprocess_time = start_time.elapsed();

    // 去重后点数不足
    if unique_points.len() < 3 {
        return Vec::new();
    }

    // Step 2: 使用 delaunator 进行三角剖分
    #[cfg(debug_assertions)]
    let triangulation_start = std::time::Instant::now();

    let triangles = triangulate_with_delaunator(&unique_points);

    #[cfg(debug_assertions)]
    {
        let triangulation_time = triangulation_start.elapsed();
        let duration = start_time.elapsed();
        print_debug_info(
            points.len(),
            unique_points.len(),
            triangles.len(),
            duration,
            preprocess_time,
            triangulation_time,
            &unique_points,
        );
    }

    // Step 3: 将索引映射回原始点数组
    map_indices_to_original(&triangles, &original_indices)
}

/// 执行 Delaunay 三角剖分并返回半边网格
///
/// 这是推荐的方式，返回的 `DelaunayMesh` 包含完整的拓扑信息，
/// 支持 O(1) 邻接查询和有序的 Voronoi 单元格遍历。
///
/// # 参数
/// - `points`: 输入点坐标列表
///
/// # 返回值
/// 包含半边结构的 Delaunay 网格
///
/// # 优势
/// - O(1) 邻接三角形查询
/// - 顶点周围边的高效遍历
/// - Voronoi 单元格顶点自然有序（无需额外排序）
///
/// # 示例
/// ```ignore
/// let points = vec![
///     Pos2::new(0.0, 0.0),
///     Pos2::new(1.0, 0.0),
///     Pos2::new(0.5, 1.0),
/// ];
///
/// let mesh = triangulate_mesh(points);
///
/// // 获取有序的 Voronoi 单元格顶点
/// for v in 0..mesh.point_count() as u32 {
///     let (vertices, is_closed) = mesh.voronoi_cell_vertices(v);
///     // vertices 已经是逆时针有序的
/// }
/// ```
pub fn triangulate_mesh(points: Vec<Pos2>) -> crate::delaunay::half_edge::DelaunayMesh {
    use crate::delaunay::half_edge::DelaunayMesh;

    if points.len() < 3 {
        return DelaunayMesh::new();
    }

    // 转换为 delaunator 格式
    let delaunay_points: Vec<delaunator::Point> = points
        .iter()
        .map(|p| delaunator::Point {
            x: p.x as f64,
            y: p.y as f64,
        })
        .collect();

    // 执行三角剖分
    let triangulation = delaunator::triangulate(&delaunay_points);

    // 构建半边网格
    DelaunayMesh::from_delaunator(points, &triangulation)
}

// ============================================================================
// 内部实现
// ============================================================================

/// 坐标量化精度（用于去重）
const COORD_QUANTIZATION: f32 = 1000.0;

/// 预处理点集合：去除重复点
///
/// 使用并行计算和整数量化来高效去重。
///
/// # 返回值
/// - `unique_points`: 去重后的点列表
/// - `original_indices`: 去重点对应的原始索引（u32）
fn preprocess_points(points: &[Pos2]) -> (Vec<Pos2>, Vec<u32>) {
    // 并行计算每个点的量化键
    #[cfg(not(target_arch = "wasm32"))]
    let iter = points.par_iter().enumerate();
    #[cfg(target_arch = "wasm32")]
    let iter = points.iter().enumerate();
    let mut point_data: Vec<_> = iter
        .map(|(idx, p)| {
            // 使用整数坐标键减少浮点误差
            let key = (
                (p.x * COORD_QUANTIZATION).round() as i32,
                (p.y * COORD_QUANTIZATION).round() as i32,
            );
            (key, idx as u32, *p)
        })
        .collect();

    // 按键值排序，便于去重
    point_data.sort_unstable_by_key(|&(key, _, _)| key);

    // 去重并保留原始索引
    let mut unique_points = Vec::with_capacity(point_data.len());
    let mut original_indices = Vec::with_capacity(point_data.len());
    let mut current_key = None;

    for (key, orig_idx, point) in point_data {
        if current_key != Some(key) {
            current_key = Some(key);
            unique_points.push(point);
            original_indices.push(orig_idx);
        }
    }

    // 压缩容量
    unique_points.shrink_to_fit();
    original_indices.shrink_to_fit();

    (unique_points, original_indices)
}

/// 使用 delaunator 库进行三角剖分
///
/// 使用线程本地缓存减少内存分配。
fn triangulate_with_delaunator(points: &[Pos2]) -> Vec<[u32; 3]> {
    if points.len() < 3 {
        return Vec::new();
    }

    // 线程本地缓存，避免重复分配
    thread_local! {
        static POINTS_CACHE: std::cell::RefCell<Vec<delaunator::Point>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    POINTS_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.clear();
        cache.reserve(points.len());

        // 转换为 delaunator 格式
        for p in points {
            cache.push(delaunator::Point {
                x: p.x as f64,
                y: p.y as f64,
            });
        }

        // 执行三角剖分
        let result = delaunator::triangulate(&cache);

        // 转换输出格式（usize -> u32）
        let mut triangles = Vec::with_capacity(result.triangles.len() / 3);
        for i in (0..result.triangles.len()).step_by(3) {
            if i + 2 < result.triangles.len() {
                triangles.push([
                    result.triangles[i] as u32,
                    result.triangles[i + 1] as u32,
                    result.triangles[i + 2] as u32,
                ]);
            }
        }

        triangles
    })
}

/// 将去重后的索引映射回原始点数组
fn map_indices_to_original(triangles: &[[u32; 3]], original_indices: &[u32]) -> Vec<u32> {
    let mut result = Vec::with_capacity(triangles.len() * 3);

    for triangle in triangles {
        result.push(original_indices[triangle[0] as usize]);
        result.push(original_indices[triangle[1] as usize]);
        result.push(original_indices[triangle[2] as usize]);
    }

    result
}

/// 打印调试信息
#[cfg(debug_assertions)]
fn print_debug_info(
    _original_count: usize,
    unique_count: usize,
    triangle_count: usize,
    total_duration: std::time::Duration,
    preprocess_time: std::time::Duration,
    triangulation_time: std::time::Duration,
    unique_points: &[Pos2],
) {
    println!("去重后剩余 {} 个点", unique_count);
    println!(
        "三角剖分完成，生成 {} 个三角形，耗时 {:.2?}",
        triangle_count, total_duration
    );
    println!("  预处理时间: {:.2?}", preprocess_time);
    println!("  三角剖分时间: {:.2?}", triangulation_time);

    // 计算凸包边界点数
    let hull_points = calculate_convex_hull_indices(unique_count, unique_points);
    println!("凸包边界点数量: {}", hull_points);

    // 理论三角形数量: 2n - 2 - k (n=点数, k=凸包点数)
    let theoretical = if unique_count >= 3 {
        2 * unique_count as i32 - 2 - hull_points
    } else {
        0
    };

    println!("理论三角形数量: {}", theoretical);
    if theoretical > 0 {
        println!(
            "实际/理论比率: {:.2}",
            triangle_count as f32 / theoretical as f32
        );
    }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_triangulation_basic() {
        let points = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 0.0),
            Pos2::new(10.0, 10.0),
            Pos2::new(0.0, 10.0),
        ];

        let indices = triangulate(&points);

        println!("四边形三角剖分:");
        println!("  输入点: {:?}", points);
        println!("  输出索引: {:?}", indices);

        // 四个点应该生成2个三角形，共6个索引
        assert_eq!(indices.len(), 6, "应该生成6个索引(2个三角形)");

        // 验证索引类型是 u32
        let _: Vec<u32> = indices;
    }

    #[test]
    fn test_empty_points() {
        let points: Vec<Pos2> = vec![];
        let indices = triangulate(&points);
        assert!(indices.is_empty());
    }

    #[test]
    fn test_too_few_points() {
        let points = vec![Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)];
        let indices = triangulate(&points);
        assert!(indices.is_empty());
    }

    #[test]
    fn test_duplicate_points() {
        let points = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(0.0, 0.0), // 重复
            Pos2::new(1.0, 0.0),
            Pos2::new(0.5, 1.0),
        ];

        let indices = triangulate(&points);
        // 去重后3个点，1个三角形
        assert_eq!(indices.len(), 3);
    }

    #[test]
    fn test_index_range() {
        // 验证索引值在有效范围内
        let points = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(0.5, 1.0),
        ];

        let indices = triangulate(&points);
        for idx in &indices {
            assert!(*idx < points.len() as u32, "索引应该在有效范围内");
        }
    }
}

// ============================================================================
// 性能基准测试
// ============================================================================

#[cfg(test)]
mod bench {
    use super::*;
    use rand::Rng;
    use std::time::Instant;

    fn generate_random_points(count: usize) -> Vec<Pos2> {
        let mut rng = rand::rng();
        (0..count)
            .map(|_| Pos2::new(rng.random_range(0.0..1000.0), rng.random_range(0.0..1000.0)))
            .collect()
    }

    fn generate_grid_points(width: usize, height: usize) -> Vec<Pos2> {
        let mut points = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                points.push(Pos2::new(x as f32 * 10.0, y as f32 * 10.0));
            }
        }
        points
    }

    #[test]
    fn benchmark_random_points() {
        println!("\n=== 随机点性能测试 ===");

        for &size in &[100, 500, 1000, 5000, 10000] {
            let points = generate_random_points(size);
            let start = Instant::now();
            let indices = triangulate(&points);
            let duration = start.elapsed();

            println!(
                "随机 {} 点: {} 三角形, 耗时 {:.2?}",
                size,
                indices.len() / 3,
                duration
            );
        }
    }

    #[test]
    fn benchmark_grid_points() {
        println!("\n=== 网格点性能测试 ===");

        for &(w, h) in &[(10, 10), (20, 20), (50, 50), (70, 70)] {
            let points = generate_grid_points(w, h);
            let start = Instant::now();
            let indices = triangulate(&points);
            let duration = start.elapsed();

            println!(
                "网格 {}x{} ({} 点): {} 三角形, 耗时 {:.2?}",
                w,
                h,
                points.len(),
                indices.len() / 3,
                duration
            );
        }
    }
}
