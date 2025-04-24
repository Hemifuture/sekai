use crate::delaunay::triangle::Triangle;
use crate::delaunay::utils::create_super_triangle;
use egui::Pos2;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// 执行Delaunay三角剖分，根据输入点集合返回三角形列表
pub fn triangulate(points: &[Pos2]) -> Vec<Triangle> {
    let start_time = Instant::now();
    println!("三角剖分开始，处理 {} 个点", points.len());

    // 至少需要3个点才能形成三角形
    if points.len() < 3 {
        return Vec::new();
    }

    // 去除重复点 - 使用HashSet提高效率
    let mut unique_points_set = HashSet::new();
    let mut unique_points = Vec::with_capacity(points.len());

    for &point in points {
        // 使用点坐标的近似值作为键
        let key = (
            (point.x * 1000.0).round() as i32,
            (point.y * 1000.0).round() as i32,
        );
        if unique_points_set.insert(key) {
            unique_points.push(point);
        }
    }

    // 如果去重后点数量不足，返回空
    if unique_points.len() < 3 {
        return Vec::new();
    }

    println!("去重后剩余 {} 个点", unique_points.len());

    // 找到能包含所有点的超级三角形
    let super_triangle = create_super_triangle(&unique_points);
    let super_points = [
        super_triangle.points[0],
        super_triangle.points[1],
        super_triangle.points[2],
    ];

    // 使用简单的Bowyer-Watson算法实现Delaunay三角剖分
    // 从超级三角形开始
    let mut triangles = vec![super_triangle];

    // 逐个添加点
    for &point in &unique_points {
        // 找出包含点在外接圆内的所有三角形 - 并行处理
        let bad_triangles: Vec<usize> = (0..triangles.len())
            .into_par_iter()
            .filter(|&i| triangles[i].contains_in_circumcircle(point))
            .collect();

        // 如果没有找到不合法的三角形，跳过此点
        if bad_triangles.is_empty() {
            continue;
        }

        // 提取多边形边界
        let mut boundary_edges = Vec::new();

        for &bad_idx in &bad_triangles {
            let triangle = triangles[bad_idx];

            // 添加三角形的三条边
            for i in 0..3 {
                let edge = [triangle.points[i], triangle.points[(i + 1) % 3]];

                // 检查这条边是否在其他不合法三角形中出现
                let mut is_shared = false;

                for &other_idx in &bad_triangles {
                    if other_idx == bad_idx {
                        continue;
                    }

                    let other_triangle = triangles[other_idx];

                    // 检查边是否在other_triangle中
                    for j in 0..3 {
                        let other_edge =
                            [other_triangle.points[j], other_triangle.points[(j + 1) % 3]];

                        // 检查边是否相同（考虑方向）
                        if (edge[0] == other_edge[0] && edge[1] == other_edge[1])
                            || (edge[0] == other_edge[1] && edge[1] == other_edge[0])
                        {
                            is_shared = true;
                            break;
                        }
                    }

                    if is_shared {
                        break;
                    }
                }

                if !is_shared {
                    boundary_edges.push(edge);
                }
            }
        }

        // 从三角形列表中移除不合法的三角形
        // 倒序移除以保持索引有效
        let mut bad_triangles_sorted = bad_triangles.clone();
        bad_triangles_sorted.sort_unstable();
        for i in bad_triangles_sorted.iter().rev() {
            triangles.swap_remove(*i);
        }

        // 用point和多边形边界创建新的三角形
        for edge in boundary_edges {
            triangles.push(Triangle::new([edge[0], edge[1], point]));
        }
    }

    // 移除与超级三角形相关的三角形 - 并行处理
    let triangles: Vec<Triangle> = triangles
        .into_par_iter()
        .filter(|t| !super_points.iter().any(|&p| t.points.contains(&p)))
        .collect();

    // 修复潜在的非Delaunay三角形
    let triangles = fix_non_delaunay_triangles(triangles);

    let duration = start_time.elapsed();
    println!(
        "三角剖分完成，生成 {} 个三角形，耗时 {:.2?}",
        triangles.len(),
        duration
    );

    triangles
}

/// 修复非Delaunay三角形
fn fix_non_delaunay_triangles(mut triangles: Vec<Triangle>) -> Vec<Triangle> {
    let mut modified = true;
    let max_iterations = 5; // 限制迭代次数防止无限循环
    let mut iteration = 0;

    while modified && iteration < max_iterations {
        iteration += 1;
        modified = false;

        // 创建边到三角形的映射 - 并行收集所有边
        let edge_to_triangles: HashMap<((i32, i32), (i32, i32)), Vec<usize>> = {
            // 先并行收集所有三角形的边
            let edges_with_indices: Vec<(((i32, i32), (i32, i32)), usize)> = triangles
                .par_iter()
                .enumerate()
                .flat_map(|(i, triangle)| {
                    let mut triangle_edges = Vec::with_capacity(3);
                    for j in 0..3 {
                        let k = (j + 1) % 3;
                        let p1 = triangle.points[j];
                        let p2 = triangle.points[k];

                        // 规范化边的表示
                        let edge = if p1.x < p2.x || (p1.x == p2.x && p1.y < p2.y) {
                            (
                                (
                                    (p1.x * 1000.0).round() as i32,
                                    (p1.y * 1000.0).round() as i32,
                                ),
                                (
                                    (p2.x * 1000.0).round() as i32,
                                    (p2.y * 1000.0).round() as i32,
                                ),
                            )
                        } else {
                            (
                                (
                                    (p2.x * 1000.0).round() as i32,
                                    (p2.y * 1000.0).round() as i32,
                                ),
                                (
                                    (p1.x * 1000.0).round() as i32,
                                    (p1.y * 1000.0).round() as i32,
                                ),
                            )
                        };
                        triangle_edges.push((edge, i));
                    }
                    triangle_edges
                })
                .collect();

            // 然后按边分组
            let mut map: HashMap<((i32, i32), (i32, i32)), Vec<usize>> = HashMap::new();
            for (edge, i) in edges_with_indices {
                map.entry(edge).or_default().push(i);
            }
            map
        };

        // 检查并翻转非Delaunay边 - 收集需要翻转的边
        let flips: Vec<(usize, usize, Pos2, Pos2, Pos2, Pos2)> = edge_to_triangles
            .par_iter()
            .filter_map(|(_, triangle_indices)| {
                if triangle_indices.len() == 2 {
                    let t1_idx = triangle_indices[0];
                    let t2_idx = triangle_indices[1];

                    let t1 = triangles[t1_idx];
                    let t2 = triangles[t2_idx];

                    // 找出共享边的非共享点
                    let mut p1_idx = None;
                    let mut p2_idx = None;

                    for i in 0..3 {
                        if !t2.points.contains(&t1.points[i]) {
                            p1_idx = Some(i);
                            break;
                        }
                    }

                    for i in 0..3 {
                        if !t1.points.contains(&t2.points[i]) {
                            p2_idx = Some(i);
                            break;
                        }
                    }

                    if let (Some(i1), Some(i2)) = (p1_idx, p2_idx) {
                        let p1 = t1.points[i1];
                        let p2 = t2.points[i2];

                        // 找出共享边的两个点
                        let shared_points: Vec<Pos2> = t1
                            .points
                            .iter()
                            .filter(|&&p| t2.points.contains(&p))
                            .cloned()
                            .collect();

                        if shared_points.len() == 2 {
                            let e1 = shared_points[0];
                            let e2 = shared_points[1];

                            // 检查是否需要翻转边
                            if t2.contains_in_circumcircle(p1) || t1.contains_in_circumcircle(p2) {
                                return Some((t1_idx, t2_idx, p1, p2, e1, e2));
                            }
                        }
                    }
                }
                None
            })
            .collect();

        // 执行翻转
        if !flips.is_empty() {
            modified = true;
            for (t1_idx, t2_idx, p1, p2, e1, e2) in flips {
                triangles[t1_idx] = Triangle::new([p1, p2, e1]);
                triangles[t2_idx] = Triangle::new([p1, p2, e2]);
            }
        }
    }

    triangles
}
