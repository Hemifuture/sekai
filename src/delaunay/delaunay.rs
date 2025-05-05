use crate::delaunay::utils::calculate_convex_hull_indices;
use egui::Pos2;
use rand::thread_rng;
use rand::Rng;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Once;
use std::time::Instant;

static INIT_LOGGER: Once = Once::new();

/// 执行Delaunay三角剖分，根据输入点集合返回三角形列表
pub fn triangulate(points: &Vec<Pos2>) -> Vec<u32> {
    let start_time = Instant::now();

    // 使用Once确保日志只初始化一次
    INIT_LOGGER.call_once(|| {
        println!("三角剖分开始，处理 {} 个点", points.len());
    });

    // 至少需要3个点才能形成三角形
    if points.len() < 3 {
        return Vec::new();
    }

    // 优化点的预处理，使用并行处理去重
    let (unique_points, original_indices) = preprocess_points(points);
    let unique_points_count = unique_points.len();

    // 记录预处理时间
    let preprocess_time = start_time.elapsed();

    // 如果去重后点数量不足，返回空
    if unique_points_count < 3 {
        return Vec::new();
    }

    // 使用delaunator进行三角剖分
    let triangulation_start = Instant::now();
    let triangles = classic_delaunay(&unique_points);
    let triangulation_time = triangulation_start.elapsed();

    let duration = start_time.elapsed();

    // 仅在调试模式下输出详细信息
    #[cfg(debug_assertions)]
    {
        println!("去重后剩余 {} 个点", unique_points_count);
        println!(
            "三角剖分完成，生成 {} 个三角形，耗时 {:.2?}",
            triangles.len(),
            duration
        );
        println!("预处理时间: {:.2?}", preprocess_time);
        println!("三角剖分时间: {:.2?}", triangulation_time);

        // 计算凸包边界点数
        let convex_hull_points = calculate_convex_hull_indices(unique_points_count, &unique_points);
        println!("凸包边界点数量: {}", convex_hull_points);

        // 理论上，对于n个点（其中k个在凸包边界上），平面Delaunay三角剖分产生的三角形数为2n-2-k
        let theoretical_triangles = if unique_points_count >= 3 {
            2 * unique_points_count as i32 - 2 - convex_hull_points
        } else {
            0
        };

        println!("理论三角形数量: {}", theoretical_triangles);
        println!(
            "实际/理论比率: {:.2}",
            triangles.len() as f32 / theoretical_triangles as f32
        );
    }

    // 将三角形索引列表转换为原始点索引列表
    let mut result = Vec::with_capacity(triangles.len() * 3);
    for triangle in triangles {
        result.push(original_indices[triangle[0] as usize]);
        result.push(original_indices[triangle[1] as usize]);
        result.push(original_indices[triangle[2] as usize]);
    }

    result
}

/// 预处理点集合，去除重复点并使用并行计算
fn preprocess_points(points: &[Pos2]) -> (Vec<Pos2>, Vec<u32>) {
    // 使用并行计算加快处理
    let point_data: Vec<_> = points
        .par_iter()
        .enumerate()
        .map(|(idx, p)| {
            // 使用整数坐标键减少浮点误差
            let key = ((p.x * 1000.0).round() as i32, (p.y * 1000.0).round() as i32);
            (key, idx, *p)
        })
        .collect();

    // 对键值排序，便于去重
    let mut sorted_point_data = point_data;
    sorted_point_data.sort_unstable_by_key(|&(key, _, _)| key);

    // 去重并保留原始索引
    let mut unique_points = Vec::with_capacity(sorted_point_data.len());
    let mut original_indices = Vec::with_capacity(sorted_point_data.len());

    let mut current_key = None;
    for (key, orig_idx, point) in sorted_point_data {
        if current_key != Some(key) {
            current_key = Some(key);
            unique_points.push(point);
            original_indices.push(orig_idx as u32);
        }
    }

    // 压缩容量以节省内存
    unique_points.shrink_to_fit();
    original_indices.shrink_to_fit();

    (unique_points, original_indices)
}

// 缓存Delaunator点对象的创建
thread_local! {
    static POINTS_CACHE: std::cell::RefCell<Vec<delaunator::Point>> = std::cell::RefCell::new(Vec::new());
}

/// 使用delaunator库进行Delaunay三角剖分
fn classic_delaunay(points: &[Pos2]) -> Vec<[u32; 3]> {
    if points.len() < 3 {
        return Vec::new();
    }

    // 使用线程本地缓存减少内存分配
    let delaunay_points = POINTS_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.clear();
        cache.reserve(points.len());

        // 将点转换为delaunator::Point格式
        for p in points {
            cache.push(delaunator::Point {
                x: p.x as f64,
                y: p.y as f64,
            });
        }

        // 执行三角剖分
        let triangulation = delaunator::triangulate(&cache);

        // 将三角形索引转换为我们需要的格式
        let mut triangles = Vec::with_capacity(triangulation.triangles.len() / 3);
        for i in (0..triangulation.triangles.len()).step_by(3) {
            if i + 2 < triangulation.triangles.len() {
                triangles.push([
                    triangulation.triangles[i] as u32,
                    triangulation.triangles[i + 1] as u32,
                    triangulation.triangles[i + 2] as u32,
                ]);
            }
        }

        triangles
    });

    delaunay_points
}

#[test]
fn test_triangulation() {
    // 创建一个简单的四边形测试用例
    let points = vec![
        Pos2::new(0.0, 0.0),
        Pos2::new(10.0, 0.0),
        Pos2::new(10.0, 10.0),
        Pos2::new(0.0, 10.0),
    ];

    let indices = triangulate(&points);

    println!("测试用例 - 四边形三角剖分:");
    println!("输入点: {:?}", points);
    println!("输出索引: {:?}", indices);

    // 应该生成2个三角形，共6个索引
    assert_eq!(indices.len(), 6, "应该生成6个索引(2个三角形)");
}

/// 性能测试模块
#[cfg(test)]
mod bench {
    use super::*;
    use std::time::Instant;

    /// 生成随机测试数据
    fn generate_random_points(count: usize) -> Vec<Pos2> {
        let mut rng = thread_rng();
        let mut points = Vec::with_capacity(count);

        for _ in 0..count {
            points.push(Pos2::new(
                rng.gen_range(0.0..1000.0),
                rng.gen_range(0.0..1000.0),
            ));
        }

        points
    }

    /// 生成网格测试数据
    fn generate_grid_points(width: usize, height: usize) -> Vec<Pos2> {
        let mut points = Vec::with_capacity(width * height);

        for y in 0..height {
            for x in 0..width {
                points.push(Pos2::new(x as f32 * 10.0, y as f32 * 10.0));
            }
        }

        points
    }

    /// 性能测试随机数据
    #[test]
    fn benchmark_random_points() {
        println!("\n=== 随机点性能测试 ===");

        let test_sizes = [100, 500, 1000, 5000, 10000];

        for &size in &test_sizes {
            let points = generate_random_points(size);

            let start_time = Instant::now();
            let indices = triangulate(&points);
            let duration = start_time.elapsed();

            println!(
                "随机点数量: {}, 三角形数量: {}, 耗时: {:.2?}",
                size,
                indices.len() / 3,
                duration
            );
        }
    }

    /// 性能测试网格数据
    #[test]
    fn benchmark_grid_points() {
        println!("\n=== 网格点性能测试 ===");

        let test_sizes = [(10, 10), (20, 20), (30, 30), (50, 50), (70, 70)];

        for &(width, height) in &test_sizes {
            let points = generate_grid_points(width, height);
            let size = points.len();

            let start_time = Instant::now();
            let indices = triangulate(&points);
            let duration = start_time.elapsed();

            println!(
                "网格大小: {}x{} ({}点), 三角形数量: {}, 耗时: {:.2?}",
                width,
                height,
                size,
                indices.len() / 3,
                duration
            );
        }
    }

    /// 比较两种不同类型数据的性能
    #[test]
    fn compare_data_distributions() {
        println!("\n=== 数据分布比较测试 ===");

        let test_size = 1000;

        // 随机分布
        let random_points = generate_random_points(test_size);
        let start_time = Instant::now();
        let random_indices = triangulate(&random_points);
        let random_duration = start_time.elapsed();

        // 网格分布
        let grid_width = (test_size as f64).sqrt().ceil() as usize;
        let grid_height = (test_size + grid_width - 1) / grid_width;
        let grid_points = generate_grid_points(grid_width, grid_height);
        let start_time = Instant::now();
        let grid_indices = triangulate(&grid_points);
        let grid_duration = start_time.elapsed();

        println!(
            "随机分布 ({}点): 三角形数量: {}, 耗时: {:.2?}",
            test_size,
            random_indices.len() / 3,
            random_duration
        );

        println!(
            "网格分布 ({}点): 三角形数量: {}, 耗时: {:.2?}",
            grid_points.len(),
            grid_indices.len() / 3,
            grid_duration
        );

        println!(
            "性能比: 网格/随机 = {:.2}",
            grid_duration.as_secs_f64() / random_duration.as_secs_f64()
        );
    }
}
