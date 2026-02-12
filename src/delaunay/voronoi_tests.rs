#[cfg(test)]
mod voronoi_validation {
    use super::super::delaunay::triangulate;
    use super::super::voronoi::{compute_voronoi, generate_voronoi_edges};
    use egui::Pos2;
    use rand::{rng, Rng};
    use std::collections::HashSet;
    use std::time::Instant;

    /// 生成随机点集用于测试
    fn generate_random_points(n: usize, width: f32, height: f32) -> Vec<Pos2> {
        let mut rng = rng();
        let mut points = Vec::with_capacity(n);

        for _ in 0..n {
            points.push(Pos2::new(
                rng.random_range(0.0..width),
                rng.random_range(0.0..height),
            ));
        }

        points
    }

    /// 验证Voronoi图的基本属性
    fn validate_voronoi_diagram(indices: &[u32], points: &[Pos2], edges: &[[Pos2; 2]]) -> bool {
        if indices.len() < 3 || points.len() < 3 {
            return edges.is_empty(); // 点数太少，应该没有边
        }

        // 检查Voronoi边数量与Delaunay边数量的关系
        // 统计Delaunay边
        let mut checked_edges = HashSet::new();
        let mut delaunay_edges_count = 0;

        for i in 0..indices.len() / 3 {
            let i1 = indices[i * 3] as usize;
            let i2 = indices[i * 3 + 1] as usize;
            let i3 = indices[i * 3 + 2] as usize;

            // 确保索引有效
            if i1 >= points.len() || i2 >= points.len() || i3 >= points.len() {
                continue;
            }

            // 处理三角形的三条边
            for (a, b) in [(i1, i2), (i2, i3), (i3, i1)] {
                let edge_key = if a < b { (a, b) } else { (b, a) };

                if checked_edges.insert(edge_key) {
                    delaunay_edges_count += 1;
                }
            }
        }

        // 对于有边界的图，边界上的Delaunay边没有对应的Voronoi边
        // 因此，Voronoi边的数量通常小于等于Delaunay边的数量
        edges.len() <= delaunay_edges_count
    }

    #[test]
    fn test_compute_voronoi_direct() {
        // 测试直接调用compute_voronoi函数
        let points = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(0.0, 1.0),
            Pos2::new(1.0, 1.0),
        ];

        let indices = triangulate(&points);
        assert!(!indices.is_empty(), "三角剖分应该至少生成一个三角形");

        let voronoi = compute_voronoi(&indices, &points);

        // 验证voronoi图的基本结构
        assert!(!voronoi.edges.is_empty(), "应该至少生成一条Voronoi边");
        assert_eq!(
            voronoi.cells.len(),
            points.len(),
            "单元格数量应该等于点的数量"
        );

        // 验证每个单元格都有关联的site点
        for (i, cell) in voronoi.cells.iter().enumerate() {
            assert!(
                (cell.site - points[i]).length() < 1e-5,
                "单元格site点应该与输入点匹配"
            );
        }
    }

    #[test]
    fn test_voronoi_square() {
        // 测试正方形的四个顶点
        let points = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(0.0, 1.0),
            Pos2::new(1.0, 1.0),
        ];

        let indices = triangulate(&points);
        assert!(!indices.is_empty(), "三角剖分应该至少生成一个三角形");

        let voronoi_edges = generate_voronoi_edges(&indices, &points);

        // 对于正方形的四个顶点，理论上应该有一个十字形的Voronoi图
        // 由于边界处理和算法实现的差异，可能会有不同数量的边
        assert!(!voronoi_edges.is_empty(), "应该至少生成一条Voronoi边");

        assert!(
            validate_voronoi_diagram(&indices, &points, &voronoi_edges),
            "Voronoi图应该满足基本性质"
        );
    }

    #[test]
    fn test_voronoi_grid() {
        // 创建一个5x5的网格点
        let mut points = Vec::new();
        for i in 0..5 {
            for j in 0..5 {
                points.push(Pos2::new(i as f32, j as f32));
            }
        }

        let indices = triangulate(&points);
        assert!(!indices.is_empty(), "三角剖分应该至少生成一个三角形");

        let voronoi_edges = generate_voronoi_edges(&indices, &points);

        // 对于网格点，每个内部点的Voronoi单元应该是一个正方形
        // 边界上的点的Voronoi单元则会被截断
        assert!(!voronoi_edges.is_empty(), "应该至少生成一条Voronoi边");

        assert!(
            validate_voronoi_diagram(&indices, &points, &voronoi_edges),
            "Voronoi图应该满足基本性质"
        );
    }

    #[test]
    fn test_voronoi_small_random() {
        // 测试少量随机点
        let points = generate_random_points(20, 100.0, 100.0);

        let indices = triangulate(&points);
        assert!(!indices.is_empty(), "三角剖分应该至少生成一个三角形");

        let voronoi_edges = generate_voronoi_edges(&indices, &points);
        assert!(!voronoi_edges.is_empty(), "应该至少生成一条Voronoi边");

        assert!(
            validate_voronoi_diagram(&indices, &points, &voronoi_edges),
            "Voronoi图应该满足基本性质"
        );
    }

    #[test]
    fn test_voronoi_medium_random() {
        // 测试中等数量随机点
        let points = generate_random_points(1000, 1000.0, 1000.0);

        let indices = triangulate(&points);
        assert!(!indices.is_empty(), "三角剖分应该至少生成一个三角形");

        let start_time = Instant::now();
        let voronoi_edges = generate_voronoi_edges(&indices, &points);
        let duration = start_time.elapsed();

        println!("1000点Voronoi计算耗时：{:?}", duration);
        assert!(
            duration.as_millis() < 500,
            "1000点的Voronoi图应在500毫秒内计算完成"
        );

        assert!(!voronoi_edges.is_empty(), "应该至少生成一条Voronoi边");
        assert!(
            validate_voronoi_diagram(&indices, &points, &voronoi_edges),
            "Voronoi图应该满足基本性质"
        );
    }

    #[test]
    fn test_voronoi_large_random() {
        // 测试大量随机点（性能测试）
        let n = 10_000;
        let points = generate_random_points(n, 10000.0, 10000.0);

        let indices = triangulate(&points);
        assert!(!indices.is_empty(), "三角剖分应该至少生成一个三角形");

        let start_time = Instant::now();
        let voronoi_edges = generate_voronoi_edges(&indices, &points);
        let duration = start_time.elapsed();

        println!("{}点Voronoi计算耗时：{:?}", n, duration);
        assert!(
            duration.as_millis() < 2000,
            "10000点的Voronoi图应在2秒内计算完成"
        );

        assert!(!voronoi_edges.is_empty(), "应该至少生成一条Voronoi边");
        assert!(
            validate_voronoi_diagram(&indices, &points, &voronoi_edges),
            "Voronoi图应该满足基本性质"
        );
    }

    #[test]
    #[ignore] // 这个测试可能会比较耗时，所以标记为可忽略
    fn test_voronoi_extreme_large_random() {
        // 测试极大量随机点（极限性能测试）
        let n = 100_000;
        let points = generate_random_points(n, 100000.0, 100000.0);

        let indices = triangulate(&points);
        assert!(!indices.is_empty(), "三角剖分应该至少生成一个三角形");

        let start_time = Instant::now();
        let voronoi_edges = generate_voronoi_edges(&indices, &points);
        let duration = start_time.elapsed();

        println!("{}点Voronoi计算耗时：{:?}", n, duration);
        assert!(
            duration.as_secs() < 10,
            "100000点的Voronoi图应在10秒内计算完成"
        ); // 考虑到CI环境可能较慢，允许稍微宽松一些

        assert!(!voronoi_edges.is_empty(), "应该至少生成一条Voronoi边");
        assert!(
            validate_voronoi_diagram(&indices, &points, &voronoi_edges),
            "Voronoi图应该满足基本性质"
        );
    }

    #[test]
    fn test_voronoi_empty() {
        // 测试空的点集
        let points: Vec<Pos2> = Vec::new();
        let indices = triangulate(&points);

        let voronoi_edges = generate_voronoi_edges(&indices, &points);
        assert_eq!(voronoi_edges.len(), 0, "空点集应该生成空的Voronoi图");
    }

    #[test]
    fn test_voronoi_single_point() {
        // 测试单点
        let points = vec![Pos2::new(0.0, 0.0)];
        let indices = triangulate(&points);

        let voronoi_edges = generate_voronoi_edges(&indices, &points);
        assert_eq!(voronoi_edges.len(), 0, "单点应该生成空的Voronoi图");
    }

    #[test]
    fn test_voronoi_collinear_points() {
        // 测试共线的点
        let points = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(2.0, 0.0),
        ];

        let indices = triangulate(&points);
        // 共线点应该不能形成有效的三角形，所以Voronoi图应该为空
        let voronoi_edges = generate_voronoi_edges(&indices, &points);
        assert_eq!(voronoi_edges.len(), 0, "共线点应该生成空的Voronoi图");
    }

    #[test]
    fn test_voronoi_benchmark() {
        // 基准测试：测量不同大小点集的性能
        let sizes = [100, 1000, 5000];

        for &size in &sizes {
            let points = generate_random_points(size, size as f32, size as f32);
            let indices = triangulate(&points);

            let start_time = Instant::now();
            let voronoi_edges = generate_voronoi_edges(&indices, &points);
            let duration = start_time.elapsed();

            println!(
                "{} 点的Voronoi计算耗时: {:?}, 生成 {} 条边",
                size,
                duration,
                voronoi_edges.len()
            );

            // 验证结果
            assert!(!voronoi_edges.is_empty(), "应该至少生成一条Voronoi边");
            assert!(
                validate_voronoi_diagram(&indices, &points, &voronoi_edges),
                "Voronoi图应该满足基本性质"
            );
        }
    }
}
