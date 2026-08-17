use std::io::{self, Write};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use sekai::engine::BuildCancellation;
use sekai::generators::natural::evaluate_profile_surface_quality;
use sekai::generators::spatial::{
    ConservativeSurfaceMapBuilder, GeodesicVoronoiBuilder, ProfileSurfaceBuildError,
    ProfileSurfaceBuilder,
};
use sekai::world::natural::{NaturalQualityProfile, QualityMetricStatus};
use sekai::world::{Meters, SphericalSpaceSpec};
use serde::Serialize;

const RADIUS_M: f64 = 6_371_000.0;

#[derive(Serialize)]
struct PerformanceEvidence {
    schema_version: u16,
    profiles: Vec<ProfilePerformance>,
    high_cancellation_latency_micros: u128,
}

#[derive(Serialize)]
struct ProfilePerformance {
    profile: NaturalQualityProfile,
    plan_resolution_micros: u128,
    authoritative_surface_micros: u128,
    control_surface_micros: u128,
    conservative_map_micros: u128,
    quality_evaluation_micros: u128,
    authoritative_cells: usize,
    control_cells: usize,
    overlap_count: usize,
    exact_json_bytes: usize,
    retained_payload_bytes_lower_bound: usize,
}

#[test]
#[ignore = "release-only Draft/Standard/High component timing and cancellation evidence"]
fn measure_profile_surface_components_and_cancellation() {
    let mut records = Vec::new();
    for profile in [
        NaturalQualityProfile::Draft,
        NaturalQualityProfile::Standard,
        NaturalQualityProfile::High,
    ] {
        let plan_started = Instant::now();
        let authoritative_spec = SphericalSpaceSpec {
            radius: Meters::new(RADIUS_M).unwrap(),
            target_cell_count: profile.authoritative_target_cell_count(),
        };
        let plan = profile.resolve(&authoritative_spec).unwrap();
        let plan_elapsed = plan_started.elapsed();

        let authoritative_started = Instant::now();
        let authoritative =
            GeodesicVoronoiBuilder::build(&plan.authoritative_space_spec()).unwrap();
        let authoritative_elapsed = authoritative_started.elapsed();
        let control_started = Instant::now();
        let control = GeodesicVoronoiBuilder::build(&plan.tectonic_control_space_spec()).unwrap();
        let control_elapsed = control_started.elapsed();
        let map_started = Instant::now();
        let map = ConservativeSurfaceMapBuilder::build(&control, &authoritative).unwrap();
        let map_elapsed = map_started.elapsed();
        let quality_started = Instant::now();
        let quality = evaluate_profile_surface_quality(&authoritative, &control, &map).unwrap();
        let quality_elapsed = quality_started.elapsed();

        assert_eq!(
            authoritative.cells().len(),
            plan.authoritative_resolved_cell_count() as usize
        );
        assert_eq!(
            control.cells().len(),
            plan.tectonic_control_resolved_cell_count() as usize
        );
        assert!(map.solve_stats().max_source_margin_relative_error() <= 1.0e-10);
        assert!(map.solve_stats().max_target_margin_relative_error() <= 1.0e-10);
        assert!(quality
            .metrics()
            .iter()
            .all(|metric| metric.status() == QualityMetricStatus::Pass));

        let exact_json_bytes = serialized_len(&plan)
            + serialized_len(&authoritative)
            + serialized_len(&control)
            + serialized_len(&map)
            + serialized_len(&quality);
        let retained_payload_bytes_lower_bound = std::mem::size_of_val(&plan)
            + spherical_payload_bytes(&authoritative)
            + spherical_payload_bytes(&control)
            + conservative_map_payload_bytes(&map)
            + quality_payload_bytes(&quality);
        eprintln!(
            "profile={profile:?} authoritative_cells={} control_cells={} overlaps={} plan={plan_elapsed:?} authoritative={authoritative_elapsed:?} control={control_elapsed:?} map={map_elapsed:?} quality={quality_elapsed:?} json_bytes={exact_json_bytes} retained_payload_lower_bound={retained_payload_bytes_lower_bound}",
            authoritative.cells().len(),
            control.cells().len(),
            map.overlap_count(),
        );
        records.push(ProfilePerformance {
            profile,
            plan_resolution_micros: plan_elapsed.as_micros(),
            authoritative_surface_micros: authoritative_elapsed.as_micros(),
            control_surface_micros: control_elapsed.as_micros(),
            conservative_map_micros: map_elapsed.as_micros(),
            quality_evaluation_micros: quality_elapsed.as_micros(),
            authoritative_cells: authoritative.cells().len(),
            control_cells: control.cells().len(),
            overlap_count: map.overlap_count(),
            exact_json_bytes,
            retained_payload_bytes_lower_bound,
        });
    }

    let cancellation_latency = measure_high_cancellation_latency();
    eprintln!("high cancellation latency={cancellation_latency:?}");
    let evidence = PerformanceEvidence {
        schema_version: 1,
        profiles: records,
        high_cancellation_latency_micros: cancellation_latency.as_micros(),
    };
    let bytes = serde_json::to_vec_pretty(&evidence).unwrap();
    write_evidence("performance.json", &bytes);
    eprintln!(
        "wrote performance.json bytes={} hash={}",
        bytes.len(),
        blake3::hash(&bytes).to_hex()
    );
}

fn measure_high_cancellation_latency() -> Duration {
    let cancellation = BuildCancellation::new();
    let worker_cancellation = cancellation.clone();
    let started = Arc::new(Barrier::new(2));
    let worker_started = Arc::clone(&started);
    let worker = std::thread::spawn(move || {
        worker_started.wait();
        ProfileSurfaceBuilder::build(
            NaturalQualityProfile::High,
            Meters::new(RADIUS_M).unwrap(),
            &worker_cancellation,
        )
    });
    started.wait();
    std::thread::sleep(Duration::from_millis(20));
    let cancellation_started = Instant::now();
    cancellation.cancel();
    let result = worker.join().unwrap();
    let latency = cancellation_started.elapsed();
    assert!(matches!(result, Err(ProfileSurfaceBuildError::Cancelled)));
    latency
}

fn serialized_len(value: &impl Serialize) -> usize {
    let mut writer = CountingWriter::default();
    serde_json::to_writer(&mut writer, value).unwrap();
    writer.bytes
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(buffer.len())
            .expect("supported serialized profile evidence fits usize");
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn spherical_payload_bytes(surface: &sekai::world::spatial::SphericalSurfaceSnapshot) -> usize {
    std::mem::size_of_val(surface)
        + std::mem::size_of_val(surface.vertices())
        + std::mem::size_of_val(surface.cells())
        + std::mem::size_of_val(surface.edges())
        + surface
            .cells()
            .iter()
            .map(|cell| {
                cell.boundary_vertices.len() * std::mem::size_of::<sekai::world::SurfaceVertexId>()
                    + cell.boundary_edges.len() * std::mem::size_of::<sekai::world::EdgeId>()
            })
            .sum::<usize>()
}

fn conservative_map_payload_bytes(map: &sekai::world::spatial::ConservativeSurfaceMap) -> usize {
    std::mem::size_of_val(map)
        + std::mem::size_of_val(map.source_cell_areas_m2())
        + std::mem::size_of_val(map.target_cell_areas_m2())
        + std::mem::size_of_val(map.target_row_offsets())
        + std::mem::size_of_val(map.weights())
}

fn quality_payload_bytes(report: &sekai::world::natural::NaturalQualityReport) -> usize {
    std::mem::size_of_val(report)
        + std::mem::size_of_val(report.metrics())
        + report
            .metrics()
            .iter()
            .map(|metric| {
                metric.id().namespace().len()
                    + metric.id().name().len()
                    + metric.reason().map_or(0, str::len)
            })
            .sum::<usize>()
}

fn write_evidence(file_name: &str, bytes: &[u8]) {
    let directory = std::path::Path::new("target")
        .join("natural-quality")
        .join("p1");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(file_name), bytes).unwrap();
}
