use std::time::{Duration, Instant};

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    solve_physical_sea_level, CoastalExchange, CoastalInputs, FormationSeaLevelSolver,
    IsostasyGenerationError, LocalAiryIsostasy,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, ProfileSurfaceBuilder};
use sekai::world::natural::{
    LandOceanKind, NaturalQualityProfile, SedimentSourceKind, SedimentSourceKindField,
    SurfaceWaterField, SurfaceWaterKind, FORMATION_AIRY_MANTLE_DENSITY_KG_M3,
    FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{Meters, SphericalSpaceSpec};

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
    water: SurfaceWaterField,
    erodibility: Vec<f32>,
    sediment_thickness_m: Vec<f32>,
    density_kg_m3: Vec<f32>,
    sources: SedimentSourceKindField,
    wind_m_s: Vec<[[f32; 3]; 12]>,
    current_m_s: Vec<[[f32; 3]; 12]>,
}

impl CoastFields {
    fn inputs(&self) -> CoastalInputs<'_> {
        CoastalInputs {
            elevation_m: &self.elevation_m,
            sea_level_m: 0.0,
            surface_water: &self.water,
            substrate_erodibility: &self.erodibility,
            sediment_thickness_m: &self.sediment_thickness_m,
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
    let mut water = vec![SurfaceWaterKind::Ocean; count];
    water[land] = SurfaceWaterKind::DryLand;
    let mut elevation_m = vec![-20.0; count];
    elevation_m[land] = 100.0;
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
            water: SurfaceWaterField::from_kinds(water),
            erodibility: vec![0.8; count],
            sediment_thickness_m: vec![0.0; count],
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
    inland.water =
        SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::DryLand; surface.cells().len()]);
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
        water.land_ocean().raw_values(),
        expected.geometry().land_ocean().raw_values()
    );
    assert_eq!(water.land_ocean().get(0), Some(LandOceanKind::Ocean));
    assert_eq!(water.land_ocean().get(1), Some(LandOceanKind::Land));
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
