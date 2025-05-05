use criterion::{black_box, criterion_group, criterion_main, Criterion};
use eframe_template::delaunay::{self, voronoi};
use egui::Pos2;
use rand::Rng;

fn generate_random_points(n: usize, width: f32, height: f32) -> Vec<Pos2> {
    let mut rng = rand::thread_rng();
    let mut points = Vec::with_capacity(n);

    for _ in 0..n {
        let x = rng.gen_range(0.0..width);
        let y = rng.gen_range(0.0..height);
        points.push(Pos2::new(x, y));
    }

    points
}

fn bench_delaunay(c: &mut Criterion) {
    let mut group = c.benchmark_group("Delaunay Triangulation");

    for &n in &[100, 1000, 10000] {
        group.bench_function(format!("triangulate_{}", n), |b| {
            let points = generate_random_points(n, 1000.0, 1000.0);
            b.iter(|| {
                black_box(delaunay::triangulate(&points));
            });
        });
    }

    group.finish();
}

fn bench_voronoi(c: &mut Criterion) {
    let mut group = c.benchmark_group("Voronoi Diagram");

    for &n in &[100, 1000, 10000] {
        group.bench_function(format!("voronoi_{}", n), |b| {
            let points = generate_random_points(n, 1000.0, 1000.0);
            let triangle_indices = delaunay::triangulate(&points);
            b.iter(|| {
                black_box(voronoi::compute_voronoi(&triangle_indices, &points));
            });
        });

        group.bench_function(format!("voronoi_edges_{}", n), |b| {
            let points = generate_random_points(n, 1000.0, 1000.0);
            let triangle_indices = delaunay::triangulate(&points);
            b.iter(|| {
                black_box(voronoi::generate_voronoi_edges(&triangle_indices, &points));
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_delaunay, bench_voronoi);
criterion_main!(benches);
