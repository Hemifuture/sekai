use std::time::{Duration, Instant};

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    implicit_stream_power_n1_height, ImplicitStreamPowerSolver, StreamPowerGenerationError,
    StreamPowerInputs,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, ProfileSurfaceBuilder};
use sekai::world::natural::{
    formation_elevation_from_components, NaturalQualityProfile, SurfaceWaterField,
    SurfaceWaterKind, CLIMATOLOGICAL_YEAR_SECONDS, ELEVATION_MAX_M,
    FORMATION_STREAM_POWER_REFERENCE_ERODIBILITY_PER_YEAR,
    FORMATION_STREAM_POWER_RUNOFF_REFERENCE_MM, FORMATION_STREAM_POWER_SLOPE_THRESHOLD,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

const IMPLICIT_TEST_STEP_YEARS: f64 = 25_000.0;

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

/// Converts a catchment area into the discharge it yields under the frozen
/// reference runoff, so a fixture states catchment size and the law still reads
/// the routed water.
fn discharge_of_reference_runoff_m3_s(area_m2: f64) -> f32 {
    (area_m2 * FORMATION_STREAM_POWER_RUNOFF_REFERENCE_MM / 1_000.0 / CLIMATOLOGICAL_YEAR_SECONDS)
        as f32
}

struct Fields {
    elevation_m: Vec<f64>,
    receiver: Vec<Option<CellId>>,
    water: SurfaceWaterField,
    discharge_m3_s: Vec<f32>,
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
            mean_annual_discharge_m3_s: &self.discharge_m3_s,
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
    let mut discharge_m3_s = vec![0.0; count];
    for (position, &cell) in path.iter().enumerate() {
        let index = cell.raw() as usize;
        elevation_m[index] = f64::from((3 - position) as u32) * 1_000.0;
        discharge_m3_s[index] = discharge_of_reference_runoff_m3_s((position + 1) as f64 * 4.0e12);
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
            discharge_m3_s,
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
        IMPLICIT_TEST_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    let second = ImplicitStreamPowerSolver::advance(
        &surface,
        fields.inputs(),
        IMPLICIT_TEST_STEP_YEARS,
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
fn discharge_erodibility_and_uplift_are_causal_while_zero_and_subthreshold_are_exact() {
    let surface = surface(42);
    let (path, base) = chain_fields(&surface);
    let head = path[0].raw() as usize;

    let mut zero = chain_fields(&surface).1;
    zero.discharge_m3_s.fill(0.0);
    let zero_result = ImplicitStreamPowerSolver::advance(
        &surface,
        zero.inputs(),
        IMPLICIT_TEST_STEP_YEARS,
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
        + edge.center_distance.get() * FORMATION_STREAM_POWER_SLOPE_THRESHOLD * 0.5;
    let subthreshold_result = ImplicitStreamPowerSolver::advance(
        &surface,
        subthreshold.inputs(),
        IMPLICIT_TEST_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(subthreshold_result.fluvial_erosion_m()[head], 0.0);

    let mut wet = chain_fields(&surface).1;
    wet.discharge_m3_s[head] *= 4.0;
    let wet_result = ImplicitStreamPowerSolver::advance(
        &surface,
        wet.inputs(),
        IMPLICIT_TEST_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    let base_result = ImplicitStreamPowerSolver::advance(
        &surface,
        base.inputs(),
        IMPLICIT_TEST_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert!(wet_result.fluvial_erosion_m()[head] > base_result.fluvial_erosion_m()[head]);

    let mut soft = chain_fields(&surface).1;
    soft.erodibility[head] = 1.0;
    let soft_result = ImplicitStreamPowerSolver::advance(
        &surface,
        soft.inputs(),
        IMPLICIT_TEST_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert!(soft_result.fluvial_erosion_m()[head] > base_result.fluvial_erosion_m()[head]);

    let mut uplift = chain_fields(&surface).1;
    uplift.uplift_rate_mm_year[head] = 1.0;
    let uplift_result = ImplicitStreamPowerSolver::advance(
        &surface,
        uplift.inputs(),
        IMPLICIT_TEST_STEP_YEARS,
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
fn submerged_cells_integrate_present_day_tectonic_forcing_without_fluvial_incision() {
    let surface = surface(42);
    let (path, mut fields) = chain_fields(&surface);
    let submerged = path[3].raw() as usize;
    assert_eq!(
        fields.water.get(submerged),
        Some(SurfaceWaterKind::Ocean),
        "the chain terminal must be the submerged base level"
    );
    fields.subsidence_rate_mm_year[submerged] = 5.0;
    let initial_elevation_m = fields.elevation_m[submerged];
    let result = ImplicitStreamPowerSolver::advance(
        &surface,
        fields.inputs(),
        IMPLICIT_TEST_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();

    // The same cell reclassified as land keeps its receiver-free terminal role,
    // so any difference in the retained displacement can only come from a
    // surface-water mask on the tectonic forcing.
    let mut exposed = fields;
    exposed.water =
        SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::DryLand; surface.cells().len()]);
    let exposed_result = ImplicitStreamPowerSolver::advance(
        &surface,
        exposed.inputs(),
        IMPLICIT_TEST_STEP_YEARS,
        &BuildCancellation::new(),
    )
    .unwrap();
    let displacement = result.tectonic_displacement_m()[submerged];
    assert_eq!(
        displacement.to_bits(),
        exposed_result.tectonic_displacement_m()[submerged].to_bits(),
        "the current tectonic forcing must not depend on the surface-water class"
    );
    assert!(
        displacement < 0.0,
        "the submerged cell must actually integrate its subsidence"
    );
    assert_eq!(
        result.elevation_m()[submerged].to_bits(),
        formation_elevation_from_components(
            initial_elevation_m,
            displacement,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .to_bits()
    );

    // Subaerial stream power still never reaches below the base level.
    assert_eq!(result.fluvial_erosion_m()[submerged], 0.0);
}

#[test]
fn tectonic_forcing_outside_the_elevation_domain_fails_instead_of_clipping() {
    let surface = surface(42);
    let mut fields = chain_fields(&surface).1;
    let cell = fields.receiver.iter().position(Option::is_none).unwrap();
    fields.elevation_m[cell] = f64::from(ELEVATION_MAX_M) - 1.0;
    fields.uplift_rate_mm_year[cell] = 1.0;

    assert!(matches!(
        ImplicitStreamPowerSolver::advance(
            &surface,
            fields.inputs(),
            2_000.0,
            &BuildCancellation::new(),
        ),
        Err(StreamPowerGenerationError::ElevationOutOfRange {
            cell: found_cell,
            found,
        }) if found_cell.raw() as usize == cell && found > f64::from(ELEVATION_MAX_M)
    ));
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
            IMPLICIT_TEST_STEP_YEARS,
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
            IMPLICIT_TEST_STEP_YEARS,
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
        discharge_m3_s: vec![discharge_of_reference_runoff_m3_s(1.0e6) as f32; count],
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
            IMPLICIT_TEST_STEP_YEARS,
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
