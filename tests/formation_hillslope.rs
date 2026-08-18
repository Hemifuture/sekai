use std::time::{Duration, Instant};

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    HillslopeGenerationError, HillslopeInputs, HillslopeWorkspace, NonlinearHillslopeTransport,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, ProfileSurfaceBuilder};
use sekai::world::natural::{
    NaturalQualityProfile, SedimentSourceKind, SedimentSourceKindField, SurfaceWaterField,
    SurfaceWaterKind, FORMATION_HILLSLOPE_CRITICAL_SLOPE,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

fn surface(radius_m: f64, target_cell_count: u32) -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(radius_m).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

struct Fields {
    elevation_m: Vec<f32>,
    water: SurfaceWaterField,
    erodibility: Vec<f32>,
    fracture: Vec<f32>,
    annual_precipitation_mm: Vec<f32>,
    substrate_density_kg_m3: Vec<f32>,
    sediment_sources: SedimentSourceKindField,
}

impl Fields {
    fn inputs(&self) -> HillslopeInputs<'_> {
        HillslopeInputs {
            elevation_m: &self.elevation_m,
            surface_water: &self.water,
            substrate_erodibility: &self.erodibility,
            fracture_intensity: &self.fracture,
            annual_precipitation_mm: &self.annual_precipitation_mm,
            substrate_density_kg_m3: &self.substrate_density_kg_m3,
            sediment_sources: &self.sediment_sources,
        }
    }
}

fn uniform_fields(count: usize, elevation_m: f32) -> Fields {
    Fields {
        elevation_m: vec![elevation_m; count],
        water: SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::DryLand; count]),
        erodibility: vec![0.5; count],
        fracture: vec![0.5; count],
        annual_precipitation_mm: vec![1_000.0; count],
        substrate_density_kg_m3: vec![2_700.0; count],
        sediment_sources: SedimentSourceKindField::from_kinds(vec![
            SedimentSourceKind::Felsic;
            count
        ]),
    }
}

fn first_edge_cells(surface: &SphericalSurfaceSnapshot) -> (CellId, CellId, f64) {
    let edge = &surface.edges()[0];
    (edge.cells[0], edge.cells[1], edge.center_distance.get())
}

#[test]
fn constant_surface_and_closed_coast_are_exact_no_ops_with_reused_workspace() {
    let surface = surface(10_000.0, 42);
    let count = surface.cells().len();
    let fields = uniform_fields(count, 100.0);
    let mut workspace = HillslopeWorkspace::default();
    let first = NonlinearHillslopeTransport::advance(
        &surface,
        fields.inputs(),
        1.0,
        &mut workspace,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(first.elevation_m(), fields.elevation_m);
    assert!(first
        .hillslope_erosion_m()
        .iter()
        .all(|&value| value == 0.0));
    assert!(first
        .hillslope_deposition_m()
        .iter()
        .all(|&value| value == 0.0));
    assert_eq!(first.transported_mass_kg(), 0.0);
    let allocation_epoch = workspace.allocation_epoch();
    let repeated = NonlinearHillslopeTransport::advance(
        &surface,
        fields.inputs(),
        1.0,
        &mut workspace,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(repeated, first);
    assert_eq!(workspace.allocation_epoch(), allocation_epoch);

    let (high, low, _) = first_edge_cells(&surface);
    let mut coast = uniform_fields(count, 0.0);
    coast.elevation_m[high.raw() as usize] = 1_000.0;
    let mut water = vec![SurfaceWaterKind::Ocean; count];
    water[high.raw() as usize] = SurfaceWaterKind::DryLand;
    water[low.raw() as usize] = SurfaceWaterKind::Ocean;
    coast.water = SurfaceWaterField::from_kinds(water);
    let coast_result = NonlinearHillslopeTransport::advance(
        &surface,
        coast.inputs(),
        10_000.0,
        &mut workspace,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(coast_result.elevation_m(), coast.elevation_m);
    assert_eq!(coast_result.transported_mass_kg(), 0.0);
}

fn isolated_edge_fields(
    surface: &SphericalSurfaceSnapshot,
    slope: f64,
    erodibility: f32,
    fracture: f32,
    precipitation_mm: f32,
) -> (CellId, CellId, Fields) {
    let count = surface.cells().len();
    let (high, low, distance_m) = first_edge_cells(surface);
    let mut fields = uniform_fields(count, 0.0);
    fields.elevation_m[high.raw() as usize] = (slope * distance_m) as f32;
    let mut water = vec![SurfaceWaterKind::Ocean; count];
    water[high.raw() as usize] = SurfaceWaterKind::DryLand;
    water[low.raw() as usize] = SurfaceWaterKind::DryLand;
    fields.water = SurfaceWaterField::from_kinds(water);
    fields.erodibility.fill(erodibility);
    fields.fracture.fill(fracture);
    fields.annual_precipitation_mm.fill(precipitation_mm);
    (high, low, fields)
}

#[test]
fn nonlinear_flux_accelerates_near_critical_slope_without_inversion() {
    let surface = surface(10_000.0, 42);
    let (low_high, low_cell, low) = isolated_edge_fields(
        &surface,
        FORMATION_HILLSLOPE_CRITICAL_SLOPE * 0.10,
        0.5,
        0.5,
        1_000.0,
    );
    let (near_high, near_cell, near) = isolated_edge_fields(
        &surface,
        FORMATION_HILLSLOPE_CRITICAL_SLOPE * 0.90,
        0.5,
        0.5,
        1_000.0,
    );
    let mut low_workspace = HillslopeWorkspace::default();
    let mut near_workspace = HillslopeWorkspace::default();
    let low_result = NonlinearHillslopeTransport::advance(
        &surface,
        low.inputs(),
        0.001,
        &mut low_workspace,
        &BuildCancellation::new(),
    )
    .unwrap();
    let near_result = NonlinearHillslopeTransport::advance(
        &surface,
        near.inputs(),
        0.001,
        &mut near_workspace,
        &BuildCancellation::new(),
    )
    .unwrap();
    let low_per_slope =
        low_result.removed_volume_m3() / (FORMATION_HILLSLOPE_CRITICAL_SLOPE * 0.10);
    let near_per_slope =
        near_result.removed_volume_m3() / (FORMATION_HILLSLOPE_CRITICAL_SLOPE * 0.90);
    assert!(near_per_slope > low_per_slope * 4.0);
    assert!(
        low_result.elevation_m()[low_high.raw() as usize]
            >= low_result.elevation_m()[low_cell.raw() as usize]
    );
    assert!(
        near_result.elevation_m()[near_high.raw() as usize]
            >= near_result.elevation_m()[near_cell.raw() as usize]
    );
}

#[test]
fn normalized_edge_flux_is_resolution_invariant_before_the_shared_limiter() {
    let slope = FORMATION_HILLSLOPE_CRITICAL_SLOPE * 0.20;
    let normalized = |target_cell_count| {
        let surface = surface(10_000.0, target_cell_count);
        let (high, low, fields) = isolated_edge_fields(&surface, slope, 0.5, 0.5, 1_000.0);
        let result = NonlinearHillslopeTransport::advance(
            &surface,
            fields.inputs(),
            0.001,
            &mut HillslopeWorkspace::default(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let edge = surface
            .edges()
            .iter()
            .find(|edge| edge.cells.contains(&high) && edge.cells.contains(&low))
            .unwrap();
        let retained_slope = (f64::from(fields.elevation_m[high.raw() as usize])
            - f64::from(fields.elevation_m[low.raw() as usize]))
            / edge.center_distance.get();
        result.removed_volume_m3() / (0.001 * edge.length.get() * retained_slope)
    };
    let coarse = normalized(42);
    let finer = normalized(162);
    assert!((coarse - finer).abs() / coarse <= 2.0e-7);

    let surface = surface(10_000.0, 42);
    let (high, low, forward) = isolated_edge_fields(&surface, slope, 0.5, 0.5, 1_000.0);
    let forward_result = NonlinearHillslopeTransport::advance(
        &surface,
        forward.inputs(),
        0.001,
        &mut HillslopeWorkspace::default(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let mut reversed = isolated_edge_fields(&surface, slope, 0.5, 0.5, 1_000.0).2;
    reversed.elevation_m[low.raw() as usize] = reversed.elevation_m[high.raw() as usize];
    reversed.elevation_m[high.raw() as usize] = 0.0;
    let reverse_result = NonlinearHillslopeTransport::advance(
        &surface,
        reversed.inputs(),
        0.001,
        &mut HillslopeWorkspace::default(),
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(
        reverse_result.removed_volume_m3().to_bits(),
        forward_result.removed_volume_m3().to_bits()
    );
}

#[test]
fn paired_transfer_closes_and_responds_to_rock_fracture_and_weathering() {
    let surface = surface(10_000.0, 42);
    let slope = FORMATION_HILLSLOPE_CRITICAL_SLOPE * 0.5;
    let run = |erodibility, fracture, precipitation_mm| {
        let (high, low, fields) =
            isolated_edge_fields(&surface, slope, erodibility, fracture, precipitation_mm);
        let result = NonlinearHillslopeTransport::advance(
            &surface,
            fields.inputs(),
            0.01,
            &mut HillslopeWorkspace::default(),
            &BuildCancellation::new(),
        )
        .unwrap();
        (high, low, fields, result)
    };
    let (high, low, fields, base) = run(0.2, 0.2, 250.0);
    let (_, _, _, weak) = run(0.0, 0.0, 0.0);
    let (_, _, _, strong) = run(1.0, 1.0, 4_000.0);
    assert!(strong.transported_mass_kg() > base.transported_mass_kg());
    assert!(base.transported_mass_kg() > weak.transported_mass_kg());
    assert_eq!(
        base.removed_mass_kg().to_bits(),
        base.deposited_mass_kg().to_bits()
    );
    assert!(base.retained_mass_relative_error() <= 2.0e-7);
    assert!(base.hillslope_erosion_m()[high.raw() as usize] > 0.0);
    assert!(base.hillslope_deposition_m()[low.raw() as usize] > 0.0);
    assert!(base.elevation_m()[high.raw() as usize] >= base.elevation_m()[low.raw() as usize]);
    assert!(base.elevation_m()[high.raw() as usize] < fields.elevation_m[high.raw() as usize]);
    assert!(base.elevation_m()[low.raw() as usize] > fields.elevation_m[low.raw() as usize]);

    let source = SedimentSourceKind::Felsic.raw() as usize;
    let deposited_source_mass = base
        .deposited_by_source_kg()
        .iter()
        .map(|channels| channels[source])
        .sum::<f64>();
    assert_eq!(
        deposited_source_mass.to_bits(),
        base.deposited_mass_kg().to_bits()
    );
    assert!(base.deposited_volume_m3() > base.removed_volume_m3());
}

#[test]
fn malformed_fields_fail_and_active_dense_work_cancels() {
    let surface = surface(10_000.0, 42);
    let mut malformed = uniform_fields(surface.cells().len(), 0.0);
    malformed.fracture.pop();
    assert!(matches!(
        NonlinearHillslopeTransport::advance(
            &surface,
            malformed.inputs(),
            1.0,
            &mut HillslopeWorkspace::default(),
            &BuildCancellation::new(),
        ),
        Err(HillslopeGenerationError::CellCountMismatch { .. })
    ));

    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface().clone();
    let fields = uniform_fields(surface.cells().len(), 100.0);
    let signal = BuildCancellation::new();
    let worker_signal = signal.clone();
    let worker = std::thread::spawn(move || {
        NonlinearHillslopeTransport::advance(
            &surface,
            fields.inputs(),
            1.0,
            &mut HillslopeWorkspace::default(),
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
        Err(HillslopeGenerationError::Cancelled)
    ));
}
