use criterion::{black_box, criterion_group, criterion_main, Criterion};
use egui::Pos2;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sekai::delaunay::{self, voronoi};

/// 使用固定种子生成随机点，确保基准测试可复现
fn generate_random_points(n: usize, width: f32, height: f32, seed: u64) -> Vec<Pos2> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut points = Vec::with_capacity(n);

    for _ in 0..n {
        let x = rng.random_range(0.0..width);
        let y = rng.random_range(0.0..height);
        points.push(Pos2::new(x, y));
    }

    points
}

/// 生成网格分布的点（模拟规则地图数据）
fn generate_grid_points(cols: usize, rows: usize, spacing: f32) -> Vec<Pos2> {
    let mut points = Vec::with_capacity(cols * rows);
    for row in 0..rows {
        for col in 0..cols {
            points.push(Pos2::new(col as f32 * spacing, row as f32 * spacing));
        }
    }
    points
}

fn bench_delaunay(c: &mut Criterion) {
    let mut group = c.benchmark_group("Delaunay Triangulation");

    // 随机分布测试
    for &n in &[100, 1000, 10000, 50000] {
        let points = generate_random_points(n, 1000.0, 1000.0, 42);
        group.bench_function(format!("random_{}", n), |b| {
            b.iter(|| {
                black_box(delaunay::triangulate(&points));
            });
        });
    }

    // 网格分布测试
    for &(cols, rows) in &[(10, 10), (32, 32), (100, 100)] {
        let points = generate_grid_points(cols, rows, 10.0);
        group.bench_function(format!("grid_{}x{}", cols, rows), |b| {
            b.iter(|| {
                black_box(delaunay::triangulate(&points));
            });
        });
    }

    group.finish();
}

fn bench_voronoi(c: &mut Criterion) {
    let mut group = c.benchmark_group("Voronoi Diagram");

    for &n in &[100, 1000, 10000, 50000] {
        // 预先计算好输入数据，不计入测量时间
        let points = generate_random_points(n, 1000.0, 1000.0, 42);
        let triangle_indices = delaunay::triangulate(&points);

        group.bench_function(format!("compute_{}", n), |b| {
            b.iter(|| {
                black_box(voronoi::compute_indexed_voronoi(&triangle_indices, &points));
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_delaunay, bench_voronoi);
criterion_main!(benches);
