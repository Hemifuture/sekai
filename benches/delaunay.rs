use criterion::{criterion_group, criterion_main, Bencher, BenchmarkId, Criterion};
use egui::Pos2;
use std::hint::black_box;

use eframe_template::delaunay::triangulate;

fn create_points(size: usize) -> Vec<Pos2> {
    (0..size)
        .map(|_| Pos2::new(rand::random(), rand::random()))
        .collect()
}

fn create_benchmark_fn(b: &mut Bencher, size: usize) {
    let points = create_points(size);
    b.iter_batched(
        || triangulate(&black_box(&points)),
        |_| {},
        criterion::BatchSize::SmallInput,
    );
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("Delaunay Triangulation");
    let iterations: [usize; 8] = [
        100, 500, 1_000, 10_000, 100_000, 500_000, 1_000_000, 2_000_000,
    ];

    for size in iterations.iter().take(3) {
        group.bench_with_input(
            BenchmarkId::new("delauney2d", size),
            size,
            |b, &bench_size| create_benchmark_fn(b, bench_size),
        );
    }

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
