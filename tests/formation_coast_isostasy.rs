use std::time::{Duration, Instant};

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    build_surface_water_geometry, solve_physical_sea_level, CoastalExchange, CoastalInputs,
    FormationSeaLevelSolver, IsostasyGenerationError, LocalAiryIsostasy,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, ProfileSurfaceBuilder};
use sekai::world::natural::{
    LandOceanKind, NaturalQualityProfile, SedimentSourceKind, SedimentSourceKindField,
    SurfaceWaterGeometry, ELEVATION_MAX_M, FORMATION_AIRY_MANTLE_DENSITY_KG_M3,
    FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR, SEDIMENT_PROVENANCE_SOURCE_COUNT,
    WATER_VOLUME_RELATIVE_TOLERANCE,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{Meters, SphericalSpaceSpec};

const WATERLINE_TRANSLATION_M: f32 = 0.181;

fn surface(radius_m: f64, target_cell_count: u32) -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(radius_m).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

struct CoastFields {
    elevation_m: Vec<f32>,
    geometry: SurfaceWaterGeometry,
    erodibility: Vec<f32>,
    sediment_thickness_m: Vec<f32>,
    sediment_provenance_fraction: Vec<[f32; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    density_kg_m3: Vec<f32>,
    sources: SedimentSourceKindField,
    wind_m_s: Vec<[[f32; 3]; 12]>,
    current_m_s: Vec<[[f32; 3]; 12]>,
}

impl CoastFields {
    fn inputs(&self) -> CoastalInputs<'_> {
        CoastalInputs {
            elevation_m: &self.elevation_m,
            surface_water_geometry: &self.geometry,
            substrate_erodibility: &self.erodibility,
            sediment_thickness_m: &self.sediment_thickness_m,
            sediment_provenance_fraction: &self.sediment_provenance_fraction,
            substrate_density_kg_m3: &self.density_kg_m3,
            sediment_sources: &self.sources,
            near_surface_wind_m_s: &self.wind_m_s,
            surface_ocean_current_m_s: &self.current_m_s,
        }
    }
}

fn exposed_coast(surface: &SphericalSurfaceSnapshot) -> (usize, CoastFields) {
    let count = surface.cells().len();
    let edge = &surface.edges()[0];
    let land = edge.cells[0].raw() as usize;
    let ocean = edge.cells[1].raw() as usize;
    let mut elevation_m = vec![-20.0; count];
    elevation_m[land] = 10.0;
    let geometry =
        build_surface_water_geometry(surface, &elevation_m, 0.0, &BuildCancellation::new())
            .unwrap();
    let mut wind_m_s = vec![[[0.0; 3]; 12]; count];
    let mut current_m_s = vec![[[0.0; 3]; 12]; count];
    let normal = edge.normal_from_first.components();
    let alongshore = cross(edge.midpoint.components(), normal);
    for month in 0..12 {
        wind_m_s[land][month] = normal.map(|value| (15.0 * value) as f32);
        current_m_s[ocean][month] = alongshore.map(|value| value as f32);
    }
    (
        land,
        CoastFields {
            elevation_m,
            geometry,
            erodibility: vec![0.8; count],
            sediment_thickness_m: vec![0.0; count],
            sediment_provenance_fraction: vec![[0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]; count],
            density_kg_m3: vec![2_700.0; count],
            sources: SedimentSourceKindField::from_kinds(vec![
                SedimentSourceKind::Volcaniclastic;
                count
            ]),
            wind_m_s,
            current_m_s,
        },
    )
}

#[test]
fn coast_requires_land_ocean_exposure_and_injects_only_removed_source_mass() {
    let surface = surface(10_000.0, 42);
    let (land, fields) = exposed_coast(&surface);
    let exposed = CoastalExchange::advance(
        &surface,
        fields.inputs(),
        1_000.0,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert!(exposed.coastal_erosion_m()[land] > 0.0);
    assert!(
        exposed.coastal_erosion_m()[land]
            <= (FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR * 1_000.0) as f32
    );
    assert!(exposed.land_exposure()[land] > 0.0);
    let injected = exposed
        .ocean_injection_by_source_kg()
        .iter()
        .flat_map(|channels| channels.iter())
        .sum::<f64>();
    assert_eq!(injected.to_bits(), exposed.produced_mass_kg().to_bits());
    assert_eq!(
        exposed.produced_by_source_kg()[2].to_bits(),
        injected.to_bits()
    );
    assert!(exposed
        .ocean_injection_by_source_kg()
        .iter()
        .enumerate()
        .all(|(index, channels)| index != land || channels.iter().all(|&mass| mass == 0.0)));

    let mut calm = exposed_coast(&surface).1;
    calm.wind_m_s.fill([[0.0; 3]; 12]);
    calm.current_m_s.fill([[0.0; 3]; 12]);
    let calm =
        CoastalExchange::advance(&surface, calm.inputs(), 1_000.0, &BuildCancellation::new())
            .unwrap();
    assert_eq!(calm.produced_mass_kg(), 0.0);

    let mut inland = exposed_coast(&surface).1;
    inland.elevation_m.fill(100.0);
    inland.geometry = build_surface_water_geometry(
        &surface,
        &inland.elevation_m,
        0.0,
        &BuildCancellation::new(),
    )
    .unwrap();
    let inland = CoastalExchange::advance(
        &surface,
        inland.inputs(),
        1_000.0,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(inland.produced_mass_kg(), 0.0);
}

#[test]
fn sediment_cover_shields_coast_without_changing_the_forcing_exposure() {
    let surface = surface(10_000.0, 42);
    let (land, bare) = exposed_coast(&surface);
    let bare_result =
        CoastalExchange::advance(&surface, bare.inputs(), 1_000.0, &BuildCancellation::new())
            .unwrap();
    let mut covered = exposed_coast(&surface).1;
    covered.sediment_thickness_m[land] = 90.0;
    covered.sediment_provenance_fraction[land][2] = 1.0;
    let covered_result = CoastalExchange::advance(
        &surface,
        covered.inputs(),
        1_000.0,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(
        bare_result.land_exposure()[land].to_bits(),
        covered_result.land_exposure()[land].to_bits()
    );
    assert!(covered_result.coastal_erosion_m()[land] < bare_result.coastal_erosion_m()[land] / 5.0);
}

#[test]
fn subcell_coast_responds_before_the_discrete_hydrology_terminal_changes() {
    let surface = surface(10_000.0, 42);
    let (land, baseline) = exposed_coast(&surface);
    let baseline_exchange = CoastalExchange::advance(
        &surface,
        baseline.inputs(),
        1_000.0,
        &BuildCancellation::new(),
    )
    .unwrap();

    let mut shifted = exposed_coast(&surface).1;
    shifted.geometry = build_surface_water_geometry(
        &surface,
        &shifted.elevation_m,
        WATERLINE_TRANSLATION_M,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(
        shifted.geometry.land_ocean().raw_values(),
        baseline.geometry.land_ocean().raw_values()
    );
    let shifted_exchange = CoastalExchange::advance(
        &surface,
        shifted.inputs(),
        1_000.0,
        &BuildCancellation::new(),
    )
    .unwrap();

    assert_ne!(
        shifted_exchange.coastal_erosion_m()[land].to_bits(),
        baseline_exchange.coastal_erosion_m()[land].to_bits()
    );
    assert_ne!(
        shifted_exchange.produced_mass_kg().to_bits(),
        baseline_exchange.produced_mass_kg().to_bits()
    );
}

#[test]
fn fixed_inventory_reproduces_the_point_181_metre_waterline_translation() {
    let surface = surface(10_000.0, 42);
    let (_, baseline) = exposed_coast(&surface);
    let inventory_m3 = baseline.geometry.total_water_volume_m3();
    let baseline_solution =
        solve_physical_sea_level(&surface, &baseline.elevation_m, inventory_m3).unwrap();

    let translation_m = WATERLINE_TRANSLATION_M;
    let translated_elevation = baseline
        .elevation_m
        .iter()
        .map(|&elevation| elevation + translation_m)
        .collect::<Vec<_>>();
    let translated =
        solve_physical_sea_level(&surface, &translated_elevation, inventory_m3).unwrap();

    let observed_translation_m = translated.sea_level_m() - baseline_solution.sea_level_m();
    let unit_roundoff = 0.5 * f32::EPSILON;
    let input_scale_m = translated_elevation
        .iter()
        .map(|value| value.abs())
        .fold(translation_m.abs(), f32::max);
    let input_addition_roundoff_m = unit_roundoff * input_scale_m;
    // One neighbor beyond round-to-nearest is at most 1.5 ulp, or 3u.
    let adjacent_candidate_roundoff_m = 3.0
        * unit_roundoff
        * (translated.sea_level_m().abs() + baseline_solution.sea_level_m().abs());
    let subtraction_roundoff_m = unit_roundoff * observed_translation_m.abs();
    let roundoff_bound_m =
        input_addition_roundoff_m + adjacent_candidate_roundoff_m + subtraction_roundoff_m;
    assert!(
        (observed_translation_m - translation_m).abs() <= roundoff_bound_m,
        "fixed-inventory translation error exceeded the f32 roundoff envelope: observed={observed_translation_m}, expected={translation_m}, bound={roundoff_bound_m}"
    );
    assert_eq!(
        translated.geometry().ocean_area_fraction(),
        baseline_solution.geometry().ocean_area_fraction()
    );
    assert_eq!(
        translated.geometry().wet_edge_fraction(),
        baseline_solution.geometry().wet_edge_fraction()
    );
    for solution in [&baseline_solution, &translated] {
        assert!(solution.relative_error() <= WATER_VOLUME_RELATIVE_TOLERANCE);
        assert_eq!(
            solution.geometry().total_water_volume_m3().to_bits(),
            inventory_m3.to_bits()
        );
    }
}

#[test]
fn airy_response_has_local_unloading_and_loading_signs_and_sea_level_closes_water() {
    let surface = surface(10_000.0, 42);
    let count = surface.cells().len();
    let mut removed_mass_kg = vec![0.0; count];
    let mut deposited_mass_kg = vec![0.0; count];
    let first_area = surface.cells()[0].area.get();
    let second_area = surface.cells()[1].area.get();
    removed_mass_kg[0] = FORMATION_AIRY_MANTLE_DENSITY_KG_M3 * first_area * 10.0;
    deposited_mass_kg[1] = FORMATION_AIRY_MANTLE_DENSITY_KG_M3 * second_area * 7.0;
    let base = vec![100.0; count];
    let airy = LocalAiryIsostasy::apply(
        &surface,
        &base,
        &removed_mass_kg,
        &deposited_mass_kg,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(airy.isostatic_response_m()[0], 10.0);
    assert_eq!(airy.isostatic_response_m()[1], -7.0);
    assert_eq!(airy.elevation_m()[0], 110.0);
    assert_eq!(airy.elevation_m()[1], 93.0);

    let mut elevation_m = vec![100.0; count];
    elevation_m[0] = 0.0;
    let inventory_m3 = first_area * 5.0;
    let water = FormationSeaLevelSolver::solve(
        &surface,
        &elevation_m,
        inventory_m3,
        &BuildCancellation::new(),
    )
    .unwrap();
    let expected = solve_physical_sea_level(&surface, &elevation_m, inventory_m3).unwrap();
    assert_eq!(
        water.sea_level_m().to_bits(),
        expected.sea_level_m().to_bits()
    );
    assert_eq!(
        water.realized_water_volume_m3().to_bits(),
        expected.realized_water_volume_m3().to_bits()
    );
    assert_eq!(
        water.geometry().land_ocean().raw_values(),
        expected.geometry().land_ocean().raw_values()
    );
    assert_eq!(
        water.geometry().land_ocean().get(0),
        Some(LandOceanKind::Ocean)
    );
    assert_eq!(
        water.geometry().land_ocean().get(1),
        Some(LandOceanKind::Land)
    );
}

#[test]
fn airy_response_outside_the_elevation_domain_fails_instead_of_clipping() {
    let surface = surface(10_000.0, 42);
    let count = surface.cells().len();
    let area_m2 = surface.cells()[0].area.get();
    let mut removed_mass_kg = vec![0.0; count];
    removed_mass_kg[0] = FORMATION_AIRY_MANTLE_DENSITY_KG_M3 * area_m2 * 2.0;
    let mut elevation_m = vec![0.0; count];
    elevation_m[0] = ELEVATION_MAX_M - 1.0;

    assert!(matches!(
        LocalAiryIsostasy::apply(
            &surface,
            &elevation_m,
            &removed_mass_kg,
            &vec![0.0; count],
            &BuildCancellation::new(),
        ),
        Err(IsostasyGenerationError::ElevationOutOfRange {
            cell,
            found,
        }) if cell.raw() == 0 && found > f64::from(ELEVATION_MAX_M)
    ));
}

#[test]
fn malformed_isostasy_fails_and_dense_work_observes_active_cancellation() {
    let surface = surface(10_000.0, 42);
    let count = surface.cells().len();
    let mut malformed = vec![0.0; count];
    malformed[0] = f64::INFINITY;
    assert!(matches!(
        LocalAiryIsostasy::apply(
            &surface,
            &vec![0.0; count],
            &malformed,
            &vec![0.0; count],
            &BuildCancellation::new(),
        ),
        Err(IsostasyGenerationError::InvalidCellValue { .. })
    ));

    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface().clone();
    let count = surface.cells().len();
    let signal = BuildCancellation::new();
    let worker_signal = signal.clone();
    let worker = std::thread::spawn(move || {
        LocalAiryIsostasy::apply(
            &surface,
            &vec![0.0; count],
            &vec![1.0; count],
            &vec![0.0; count],
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
        Err(IsostasyGenerationError::Cancelled)
    ));

    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface().clone();
    let count = surface.cells().len();
    let inventory_m3 = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get() * 1_000.0)
        .sum::<f64>();
    let signal = BuildCancellation::new();
    let worker_signal = signal.clone();
    let worker = std::thread::spawn(move || {
        let elevation = (0..count)
            .map(|index| (index % 10_000) as f32)
            .collect::<Vec<_>>();
        FormationSeaLevelSolver::solve(&surface, &elevation, inventory_m3, &worker_signal)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while signal.observation_count() < 24 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(signal.observation_count() >= 24);
    signal.cancel();
    assert!(matches!(
        worker.join().unwrap(),
        Err(IsostasyGenerationError::Cancelled)
    ));
}
