use std::time::{Duration, Instant};

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    implicit_stream_power_n1_height, ImplicitStreamPowerSolver, StreamPowerGenerationError,
    StreamPowerInputs,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, ProfileSurfaceBuilder};
use sekai::world::natural::{
    formation_elevation_from_components, NaturalQualityProfile, SurfaceWaterField,
    SurfaceWaterKind, FORMATION_STREAM_POWER_REFERENCE_ERODIBILITY_PER_YEAR,
    FORMATION_STREAM_POWER_SLOPE_THRESHOLD, SURFACE_FORMATION_MACRO_STEP_YEARS,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

fn surface(target_cell_count: u32) -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn simple_path(surface: &SphericalSurfaceSnapshot, length: usize) -> Vec<CellId> {
    fn visit(
        surface: &SphericalSurfaceSnapshot,
        cell: CellId,
        length: usize,
        path: &mut Vec<CellId>,
    ) -> bool {
        path.push(cell);
        if path.len() == length {
            return true;
        }
        let mut neighbors = surface
            .cell_edges(cell)
            .unwrap()
            .iter()
            .filter_map(|&edge| surface.opposite_cell(cell, edge))
            .collect::<Vec<_>>();
        neighbors.sort();
        for neighbor in neighbors {
            if !path.contains(&neighbor) && visit(surface, neighbor, length, path) {
                return true;
            }
        }
        path.pop();
        false
    }

    let mut path = Vec::new();
    assert!(visit(surface, CellId::from_raw(0), length, &mut path));
    path
}

struct Fields {
    elevation_m: Vec<f32>,
    receiver: Vec<Option<CellId>>,
    water: SurfaceWaterField,
    drainage_area_km2: Vec<f32>,
    annual_runoff_mm: Vec<f32>,
    uplift_rate_mm_year: Vec<f32>,
    subsidence_rate_mm_year: Vec<f32>,
    erodibility: Vec<f32>,
}

impl Fields {
    fn inputs(&self) -> StreamPowerInputs<'_> {
        StreamPowerInputs {
            elevation_m: &self.elevation_m,
            flow_receiver: &self.receiver,
            surface_water: &self.water,
            drainage_area_km2: &self.drainage_area_km2,
            annual_local_runoff_mm: &self.annual_runoff_mm,
            uplift_rate_mm_per_year: &self.uplift_rate_mm_year,
            subsidence_rate_mm_per_year: &self.subsidence_rate_mm_year,
            substrate_erodibility: &self.erodibility,
        }
    }
}

fn chain_fields(surface: &SphericalSurfaceSnapshot) -> (Vec<CellId>, Fields) {
    let count = surface.cells().len();
    let path = simple_path(surface, 4);
    let mut elevation_m = vec![0.0; count];
    let mut receiver = vec![None; count];
    let mut water_kind = vec![SurfaceWaterKind::DryLand; count];
    let mut drainage_area_km2 = vec![1.0; count];
    let mut annual_runoff_mm = vec![0.0; count];
    for (position, &cell) in path.iter().enumerate() {
        let index = cell.raw() as usize;
        elevation_m[index] = (3 - position) as f32 * 1_000.0;
        drainage_area_km2[index] = (position + 1) as f32 * 4_000_000.0;
        annual_runoff_mm[index] = 1_000.0;
        if let Some(&downstream) = path.get(position + 1) {
            receiver[index] = Some(downstream);
        } else {
            water_kind[index] = SurfaceWaterKind::Ocean;
        }
    }
    (
        path,
        Fields {
            elevation_m,
            receiver,
            water: SurfaceWaterField::from_kinds(water_kind),
            drainage_area_km2,
            annual_runoff_mm,
            uplift_rate_mm_year: vec![0.0; count],
            subsidence_rate_mm_year: vec![0.0; count],
            erodibility: vec![0.5; count],
        },
    )
}

#[test]
fn n1_backward_euler_matches_closed_form_when_explicit_update_is_unstable() {
    let forced_height_m = 2_000.0;
    let receiver_height_m = 100.0;
    let length_m = 10_000.0;
    let drainage_area_m2: f64 = 1.0e12;
    let erodibility_per_year = 1.0e-4;
    let step_years = 25_000.0;
    let threshold_height_m = receiver_height_m + length_m * FORMATION_STREAM_POWER_SLOPE_THRESHOLD;
    let c = step_years * erodibility_per_year * drainage_area_m2.sqrt() / length_m;
    let expected = (forced_height_m + c * threshold_height_m) / (1.0 + c);
    let implicit = implicit_stream_power_n1_height(
        forced_height_m,
        receiver_height_m,
        length_m,
        drainage_area_m2,
        erodibility_per_year,
        step_years,
    )
    .unwrap();
    assert!((implicit - expected).abs() <= 1.0e-12);
    assert!(implicit > threshold_height_m);

    let slope_excess =
        (forced_height_m - receiver_height_m) / length_m - FORMATION_STREAM_POWER_SLOPE_THRESHOLD;
    let explicit = forced_height_m
        - step_years * erodibility_per_year * drainage_area_m2.sqrt() * slope_excess;
    assert!(
        explicit < receiver_height_m,
        "the rejected explicit step crosses base level"
    );

    let reference_years = 0.1;
    let reference_substeps = 100_000;
    let reference_dt = reference_years / f64::from(reference_substeps);
    let mut explicit_reference = forced_height_m;
    for _ in 0..reference_substeps {
        let excess = ((explicit_reference - receiver_height_m) / length_m
            - FORMATION_STREAM_POWER_SLOPE_THRESHOLD)
            .max(0.0);
        explicit_reference -=
            reference_dt * erodibility_per_year * drainage_area_m2.sqrt() * excess;
    }
    let bounded_implicit = implicit_stream_power_n1_height(
        forced_height_m,
        receiver_height_m,
        length_m,
        drainage_area_m2,
        erodibility_per_year,
        reference_years,
    )
    .unwrap();
    assert!((bounded_implicit - explicit_reference).abs() < 0.02);
}

#[test]
fn spherical_chain_is_deterministic_monotone_base_safe_and_component_exact() {
    let surface = surface(42);
    let (path, fields) = chain_fields(&surface);
    let first = ImplicitStreamPowerSolver::advance(
        &surface,
        fields.inputs(),
        SURFACE_FORMATION_MACRO_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    let second = ImplicitStreamPowerSolver::advance(
        &surface,
        fields.inputs(),
        SURFACE_FORMATION_MACRO_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(first, second);

    for &cell in &path {
        let index = cell.raw() as usize;
        assert_eq!(
            first.elevation_m()[index].to_bits(),
            formation_elevation_from_components(
                fields.elevation_m[index],
                first.tectonic_displacement_m()[index],
                first.fluvial_erosion_m()[index],
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            )
            .to_bits()
        );
        if let Some(receiver) = fields.receiver[index] {
            assert!(first.elevation_m()[index] >= first.elevation_m()[receiver.raw() as usize]);
        }
    }
    let terminal = path[3].raw() as usize;
    assert_eq!(first.elevation_m()[terminal], fields.elevation_m[terminal]);
    assert_eq!(first.fluvial_erosion_m()[terminal], 0.0);
    assert!(first.fluvial_erosion_m()[path[0].raw() as usize] > 0.0);
}

#[test]
fn runoff_erodibility_and_uplift_are_causal_while_zero_and_subthreshold_are_exact() {
    let surface = surface(42);
    let (path, base) = chain_fields(&surface);
    let head = path[0].raw() as usize;

    let mut zero = chain_fields(&surface).1;
    zero.annual_runoff_mm.fill(0.0);
    let zero_result = ImplicitStreamPowerSolver::advance(
        &surface,
        zero.inputs(),
        SURFACE_FORMATION_MACRO_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(zero_result.elevation_m(), zero.elevation_m);
    assert!(zero_result
        .tectonic_displacement_m()
        .iter()
        .all(|&value| value == 0.0));
    assert!(zero_result
        .fluvial_erosion_m()
        .iter()
        .all(|&value| value == 0.0));

    let mut subthreshold = chain_fields(&surface).1;
    let downstream = subthreshold.receiver[head].unwrap().raw() as usize;
    let mut subthreshold_water = vec![SurfaceWaterKind::DryLand; surface.cells().len()];
    subthreshold_water[downstream] = SurfaceWaterKind::Ocean;
    subthreshold.water = SurfaceWaterField::from_kinds(subthreshold_water);
    subthreshold.receiver[downstream] = None;
    let edge = surface
        .cell_edges(path[0])
        .unwrap()
        .iter()
        .find(|&&edge| surface.opposite_cell(path[0], edge) == Some(path[1]))
        .and_then(|&edge| surface.edge(edge))
        .unwrap();
    subthreshold.elevation_m[head] = subthreshold.elevation_m[downstream]
        + (edge.center_distance.get() * FORMATION_STREAM_POWER_SLOPE_THRESHOLD * 0.5) as f32;
    let subthreshold_result = ImplicitStreamPowerSolver::advance(
        &surface,
        subthreshold.inputs(),
        SURFACE_FORMATION_MACRO_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(subthreshold_result.fluvial_erosion_m()[head], 0.0);

    let mut wet = chain_fields(&surface).1;
    wet.annual_runoff_mm[head] = 4_000.0;
    let wet_result = ImplicitStreamPowerSolver::advance(
        &surface,
        wet.inputs(),
        SURFACE_FORMATION_MACRO_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    let base_result = ImplicitStreamPowerSolver::advance(
        &surface,
        base.inputs(),
        SURFACE_FORMATION_MACRO_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert!(wet_result.fluvial_erosion_m()[head] > base_result.fluvial_erosion_m()[head]);

    let mut soft = chain_fields(&surface).1;
    soft.erodibility[head] = 1.0;
    let soft_result = ImplicitStreamPowerSolver::advance(
        &surface,
        soft.inputs(),
        SURFACE_FORMATION_MACRO_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert!(soft_result.fluvial_erosion_m()[head] > base_result.fluvial_erosion_m()[head]);

    let mut uplift = chain_fields(&surface).1;
    uplift.uplift_rate_mm_year[head] = 1.0;
    let uplift_result = ImplicitStreamPowerSolver::advance(
        &surface,
        uplift.inputs(),
        SURFACE_FORMATION_MACRO_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert!(uplift_result.elevation_m()[head] > base_result.elevation_m()[head]);
    assert!(uplift_result.tectonic_displacement_m()[head] > 0.0);
    assert_eq!(
        FORMATION_STREAM_POWER_REFERENCE_ERODIBILITY_PER_YEAR,
        5.0e-6
    );
}

#[test]
fn malformed_receivers_fail_and_active_dense_work_cancels() {
    let surface = surface(42);
    let (path, mut invalid) = chain_fields(&surface);
    let head = invalid.receiver.iter().position(Option::is_some).unwrap();
    let non_neighbor = (0..surface.cells().len())
        .map(|index| CellId::from_raw(index as u32))
        .find(|&candidate| {
            candidate.raw() as usize != head
                && !surface
                    .cell_edges(CellId::from_raw(head as u32))
                    .unwrap()
                    .iter()
                    .any(|&edge| {
                        surface.opposite_cell(CellId::from_raw(head as u32), edge)
                            == Some(candidate)
                    })
        })
        .unwrap();
    invalid.receiver[head] = Some(non_neighbor);
    assert!(matches!(
        ImplicitStreamPowerSolver::advance(
            &surface,
            invalid.inputs(),
            SURFACE_FORMATION_MACRO_STEP_YEARS,
            &BuildCancellation::new(),
        ),
        Err(StreamPowerGenerationError::ReceiverNotAdjacent { .. })
    ));

    let mut cyclic = chain_fields(&surface).1;
    cyclic.receiver[path[1].raw() as usize] = Some(path[0]);
    assert!(matches!(
        ImplicitStreamPowerSolver::advance(
            &surface,
            cyclic.inputs(),
            SURFACE_FORMATION_MACRO_STEP_YEARS,
            &BuildCancellation::new(),
        ),
        Err(StreamPowerGenerationError::ReceiverCycle)
    ));

    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface().clone();
    let count = surface.cells().len();
    let fields = Fields {
        elevation_m: vec![1_000.0; count],
        receiver: vec![None; count],
        water: SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::DryLand; count]),
        drainage_area_km2: vec![1.0; count],
        annual_runoff_mm: vec![1_000.0; count],
        uplift_rate_mm_year: vec![0.0; count],
        subsidence_rate_mm_year: vec![0.0; count],
        erodibility: vec![0.5; count],
    };
    let signal = BuildCancellation::new();
    let worker_signal = signal.clone();
    let worker = std::thread::spawn(move || {
        ImplicitStreamPowerSolver::advance(
            &surface,
            fields.inputs(),
            SURFACE_FORMATION_MACRO_STEP_YEARS,
            &worker_signal,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while signal.observation_count() < 16 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(signal.observation_count() >= 16);
    signal.cancel();
    assert!(matches!(
        worker.join().unwrap(),
        Err(StreamPowerGenerationError::Cancelled)
    ));
}
