use std::fmt::Write as _;
use std::io::{self, Write};

use sekai::engine::BuildCancellation;
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{NaturalQualityProfile, QualityMetricStatus};
use sekai::world::Meters;
use serde::Serialize;

const RADIUS_M: f64 = 6_371_000.0;

#[derive(Serialize)]
struct P1Evidence {
    schema_version: u16,
    radius_m: f64,
    profiles: Vec<ProfileEvidence>,
}

#[derive(Serialize)]
struct ProfileEvidence {
    profile: NaturalQualityProfile,
    authoritative_target_cells: u32,
    authoritative_resolved_cells: u32,
    control_target_cells: u32,
    control_resolved_cells: u32,
    climate_face_resolution: u16,
    authoritative_fingerprint: String,
    control_fingerprint: String,
    map_source_fingerprint: String,
    map_target_fingerprint: String,
    overlap_count: usize,
    balance_iterations: u16,
    max_source_margin_relative_error: f64,
    max_target_margin_relative_error: f64,
    max_relative_geometric_adjustment: f64,
    exact_json_bytes: ArtifactBytes,
    retained_payload_bytes_lower_bound: ArtifactBytes,
    metrics: Vec<MetricEvidence>,
}

#[derive(Serialize)]
struct ArtifactBytes {
    plan: usize,
    authoritative_surface: usize,
    control_surface: usize,
    conservative_map: usize,
    quality_report: usize,
    total: usize,
}

impl ArtifactBytes {
    fn new(
        plan: usize,
        authoritative_surface: usize,
        control_surface: usize,
        conservative_map: usize,
        quality_report: usize,
    ) -> Self {
        Self {
            plan,
            authoritative_surface,
            control_surface,
            conservative_map,
            quality_report,
            total: plan
                + authoritative_surface
                + control_surface
                + conservative_map
                + quality_report,
        }
    }
}

#[derive(Serialize)]
struct MetricEvidence {
    id: String,
    status: QualityMetricStatus,
    value: Option<f64>,
    sample_count: u32,
    minimum: Option<f64>,
    maximum: Option<f64>,
}

#[test]
#[ignore = "release-only deterministic Draft/Standard/High P1 evidence writer"]
fn write_deterministic_profile_surface_evidence() {
    let mut profiles = Vec::new();
    for profile in [
        NaturalQualityProfile::Draft,
        NaturalQualityProfile::Standard,
        NaturalQualityProfile::High,
    ] {
        let bundle = ProfileSurfaceBuilder::build(
            profile,
            Meters::new(RADIUS_M).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let plan = bundle.resolution_plan();
        let authoritative = bundle.authoritative_surface();
        let control = bundle.tectonic_control_surface();
        let map = bundle.control_to_authoritative_map();
        let quality = bundle.quality_report();
        let (expected_authoritative, expected_control) = expected_counts(profile);
        assert_eq!(authoritative.cells().len(), expected_authoritative);
        assert_eq!(control.cells().len(), expected_control);
        assert_eq!(map.source_ref().fingerprint(), control.fingerprint());
        assert_eq!(map.target_ref().fingerprint(), authoritative.fingerprint());
        assert!(map.solve_stats().max_source_margin_relative_error() <= 1.0e-10);
        assert!(map.solve_stats().max_target_margin_relative_error() <= 1.0e-10);
        assert_eq!(quality.metrics().len(), 8);
        assert!(quality
            .metrics()
            .iter()
            .all(|metric| metric.status() == QualityMetricStatus::Pass));

        let exact_json_bytes = ArtifactBytes::new(
            serialized_len(plan),
            serialized_len(authoritative),
            serialized_len(control),
            serialized_len(map),
            serialized_len(quality),
        );
        let retained_payload_bytes_lower_bound = ArtifactBytes::new(
            std::mem::size_of_val(plan),
            spherical_payload_bytes(authoritative),
            spherical_payload_bytes(control),
            conservative_map_payload_bytes(map),
            quality_payload_bytes(quality),
        );
        let stats = map.solve_stats();
        let metrics = quality
            .metrics()
            .iter()
            .map(|metric| MetricEvidence {
                id: format!(
                    "{}.{}.v{}",
                    metric.id().namespace(),
                    metric.id().name(),
                    metric.id().version()
                ),
                status: metric.status(),
                value: metric.value(),
                sample_count: metric.sample_count(),
                minimum: metric.bounds().min(),
                maximum: metric.bounds().max(),
            })
            .collect();
        eprintln!(
            "profile={profile:?} authoritative_fingerprint={} control_fingerprint={} overlaps={} json_bytes={} retained_payload_lower_bound={}",
            hex(authoritative.fingerprint()),
            hex(control.fingerprint()),
            map.overlap_count(),
            exact_json_bytes.total,
            retained_payload_bytes_lower_bound.total,
        );
        profiles.push(ProfileEvidence {
            profile,
            authoritative_target_cells: plan.authoritative_target_cell_count(),
            authoritative_resolved_cells: plan.authoritative_resolved_cell_count(),
            control_target_cells: plan.tectonic_control_target_cell_count(),
            control_resolved_cells: plan.tectonic_control_resolved_cell_count(),
            climate_face_resolution: plan.climate_face_resolution(),
            authoritative_fingerprint: hex(authoritative.fingerprint()),
            control_fingerprint: hex(control.fingerprint()),
            map_source_fingerprint: hex(map.source_ref().fingerprint()),
            map_target_fingerprint: hex(map.target_ref().fingerprint()),
            overlap_count: map.overlap_count(),
            balance_iterations: stats.balance_iterations(),
            max_source_margin_relative_error: stats.max_source_margin_relative_error(),
            max_target_margin_relative_error: stats.max_target_margin_relative_error(),
            max_relative_geometric_adjustment: stats.max_relative_geometric_adjustment(),
            exact_json_bytes,
            retained_payload_bytes_lower_bound,
            metrics,
        });
    }

    let evidence = P1Evidence {
        schema_version: 1,
        radius_m: RADIUS_M,
        profiles,
    };
    let first = serde_json::to_vec_pretty(&evidence).unwrap();
    let second = serde_json::to_vec_pretty(&evidence).unwrap();
    assert_eq!(first, second);
    let directory = std::path::Path::new("target")
        .join("natural-quality")
        .join("p1");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("evidence.json");
    std::fs::write(&path, &first).unwrap();
    eprintln!(
        "wrote {} bytes to {} hash={}",
        first.len(),
        path.display(),
        blake3::hash(&first).to_hex()
    );
}

fn expected_counts(profile: NaturalQualityProfile) -> (usize, usize) {
    match profile {
        NaturalQualityProfile::Draft => (20_252, 4_842),
        NaturalQualityProfile::Standard => (79_212, 20_252),
        NaturalQualityProfile::High => (198_812, 20_252),
    }
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

fn hex(bytes: [u8; 32]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing into a string cannot fail");
    }
    encoded
}
