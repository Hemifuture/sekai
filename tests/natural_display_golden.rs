use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicArtifact,
    GeologicSpecArtifact, HydroErosionArtifact, HydroErosionSpecArtifact, MantleArtifact,
    PreliminaryClimateArtifact, ReliefArtifact, RulePackSetArtifact, TectonicArtifact,
    TectonicSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::view::{
    built_in_palette, prepare_cell_field, rasterize_reference, DisplayRangeMode,
    DisplayRevisionClock, DisplayRevisions, FieldCatalog, FieldPayloadRef, MeshCompleteness,
    PaletteId, PreparedCellMesh, PreparedDiagnosticMask, PreparedFieldDisplay,
};
use sekai::world::fields::{FieldId, ValueRange};
use sekai::world::natural::{
    bedrock_kind_field_id, crust_kind_field_id, elevation_field_id,
    fluvial_erosion_depth_m_field_id, geothermal_potential_field_id, mantle_heat_flow_field_id,
    maritime_influence_field_id, metallic_mineral_potential_field_id, natural_field_registry,
    plate_id_field_id, preliminary_annual_precipitation_mm_field_id,
    preliminary_mean_air_temperature_c_field_id, preliminary_temperature_seasonality_c_field_id,
    sediment_deposition_thickness_m_field_id, sedimentary_basin_potential_field_id,
    strahler_stream_order_field_id, surface_elevation_m_field_id, surface_water_kind_field_id,
    volcanic_influence_field_id, BedrockKind, BoundaryKind, ClimateSpec, GeologicSnapshot,
    GeologicSpec, HydroErosionSnapshot, HydroErosionSpec, MantleSnapshot,
    PreliminaryClimateSnapshot, ReliefSnapshot, SurfaceWaterKind, TectonicSnapshot, TectonicSpec,
    COMPONENT_IDENTITY_TOLERANCE_M,
};
use sekai::world::spatial::{SpatialSnapshot, Topology};
use sekai::world::{BoundaryCondition, CellId, Meters, PlanarSpaceSpec, RootSeed};

const QUALITY_CELL_COUNT: u32 = 512;
const GOLDEN_SEED: u64 = 0x00C0_FFEE;
const GOLDEN_WIDTH: u32 = 256;
const GOLDEN_HEIGHT: u32 = 128;
const QUALITY_SEEDS: [u64; 8] = [
    1,
    7,
    42,
    0x5E_A1,
    0x00C0_FFEE,
    0xDEAD_BEEF,
    0x1234_5678_9ABC_DEF0,
    u64::MAX - 17,
];

#[test]
fn quality_across_fixed_seed_set() {
    run_multi_seed_quality_suite();
}

#[test]
fn reviewed_natural_goldens_match() {
    for (name, packet) in golden_packets() {
        assert_golden(name, &packet);
    }
}

#[test]
#[ignore = "writes reviewed natural-foundation golden PNGs"]
fn regenerate_natural_goldens() {
    assert_eq!(
        std::env::var("SEKAI_UPDATE_NATURAL_GOLDENS").as_deref(),
        Ok("1")
    );
    for (name, packet) in golden_packets() {
        write_golden(name, &packet);
    }
}

fn run_multi_seed_quality_suite() {
    let mut saw_mixed_crust_plate = false;
    let mut saw_cross_plate_crust_component = false;

    for seed in QUALITY_SEEDS {
        let fixture = build_natural(seed, QUALITY_CELL_COUNT);
        let spatial = fixture.spatial.snapshot();
        let tectonic = fixture.tectonic.snapshot();
        let mantle = fixture.mantle.snapshot();
        let relief = fixture.relief.snapshot();
        let geology = fixture.geology.snapshot();
        let climate = fixture.climate.snapshot();
        let hydro_erosion = fixture.hydro_erosion.snapshot();
        tectonic.validate_against(spatial).unwrap();
        mantle.validate_against(spatial).unwrap();
        relief.validate_against(spatial).unwrap();
        geology
            .validate_against(spatial, tectonic, mantle, relief)
            .unwrap();
        climate.validate_against(spatial, relief).unwrap();
        hydro_erosion
            .validate_against(spatial, relief, geology, climate)
            .unwrap();

        assert_eq!(spatial.cell_count(), QUALITY_CELL_COUNT as usize);
        assert_eq!(tectonic.cell_count(), QUALITY_CELL_COUNT);
        assert_eq!(mantle.cell_count(), QUALITY_CELL_COUNT);
        assert_eq!(relief.cell_count(), QUALITY_CELL_COUNT);
        assert_eq!(geology.cell_count(), QUALITY_CELL_COUNT);
        assert_eq!(climate.cell_count(), QUALITY_CELL_COUNT);
        assert_eq!(hydro_erosion.cell_count(), QUALITY_CELL_COUNT);
        assert_plate_connectivity_and_balance(seed, spatial, tectonic);
        assert_boundary_partition_and_motion(seed, spatial, tectonic);

        let continental_cells = tectonic
            .crust_kinds()
            .raw_values()
            .iter()
            .filter(|&&kind| kind == 1)
            .count();
        let continental_fraction = continental_cells as f32 / QUALITY_CELL_COUNT as f32;
        assert!(
            (continental_fraction - TectonicSpec::default().continental_crust_fraction).abs()
                <= 0.04,
            "seed {seed}: continental fraction {continental_fraction}"
        );

        let land_cells = relief
            .land_ocean()
            .raw_values()
            .iter()
            .filter(|&&kind| kind == 1)
            .count();
        assert!(
            land_cells > 0 && land_cells < QUALITY_CELL_COUNT as usize,
            "seed {seed}: both land and ocean are required"
        );

        saw_mixed_crust_plate |= plate_has_mixed_crust(tectonic);
        let components = crust_components(spatial, tectonic);
        saw_cross_plate_crust_component |= components.iter().any(|component| {
            component
                .iter()
                .map(|&cell| tectonic.cell_plates().raw_values()[cell])
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        });
        let largest_continental_component = components
            .iter()
            .filter(|component| tectonic.crust_kinds().raw_values()[component[0]] == 1)
            .map(Vec::len)
            .max()
            .unwrap_or(0);
        assert!(
            largest_continental_component as f32 / QUALITY_CELL_COUNT as f32 <= 0.50,
            "seed {seed}: one continental component consumed {largest_continental_component} cells"
        );

        assert_relief_finite_and_explainable(seed, relief);
        assert_geologic_quality(seed, tectonic, mantle, relief, geology);
        assert_climate_quality(seed, spatial, relief, climate);
        assert_hydro_erosion_quality(seed, spatial, hydro_erosion);
        let mesh = PreparedCellMesh::build(spatial, MeshCompleteness::RequireAll).unwrap();
        assert_eq!(mesh.cell_count(), QUALITY_CELL_COUNT as usize);
        let registry = natural_field_registry(tectonic.plates().len() as u16).unwrap();
        let catalog = FieldCatalog::from_payloads(
            &registry,
            [(
                elevation_field_id(),
                FieldPayloadRef::ScalarF32(relief.elevation_m().values()),
            )],
        )
        .unwrap();
        let view = catalog.get(&elevation_field_id()).unwrap().view().unwrap();
        let prepared =
            prepare_cell_field(view, mesh.cell_count(), DisplayRangeMode::Schema).unwrap();
        assert_eq!(prepared.len(), mesh.cell_count());

        eprintln!(
            "seed={seed} cells={} edges={} plates={} segments={} continental={continental_fraction:.3} land={:.3} mean_temp={:.2} annual_precip={:.1}",
            spatial.cell_count(),
            spatial.edges().len(),
            tectonic.plates().len(),
            tectonic.boundary_segments().len(),
            land_cells as f32 / QUALITY_CELL_COUNT as f32,
            mean(climate.mean_annual_air_temperature_c().iter().copied()),
            mean(climate.annual_precipitation_mm().iter().copied()),
        );
    }

    assert!(
        saw_mixed_crust_plate,
        "the fixed quality set must contain a plate spanning both crust kinds"
    );
    assert!(
        saw_cross_plate_crust_component,
        "the fixed quality set must contain a crust component crossing plate boundaries"
    );

    let baseline = build_natural(GOLDEN_SEED, QUALITY_CELL_COUNT);
    let changed = build_natural_with_geologic_spec(
        GOLDEN_SEED,
        QUALITY_CELL_COUNT,
        GeologicSpec {
            hotspot_count: 0,
            ..GeologicSpec::default()
        },
    );
    assert_eq!(
        serde_json::to_vec(baseline.tectonic.as_ref()).unwrap(),
        serde_json::to_vec(changed.tectonic.as_ref()).unwrap(),
        "geologic configuration must not perturb plates or crust"
    );
}

struct NaturalFixture {
    spatial: Arc<SpatialArtifact>,
    tectonic: Arc<TectonicArtifact>,
    mantle: Arc<MantleArtifact>,
    relief: Arc<ReliefArtifact>,
    geology: Arc<GeologicArtifact>,
    climate: Arc<PreliminaryClimateArtifact>,
    hydro_erosion: Arc<HydroErosionArtifact>,
}

fn build_natural(seed: u64, cell_count: u32) -> NaturalFixture {
    build_natural_with_geologic_spec(seed, cell_count, GeologicSpec::default())
}

fn build_natural_with_geologic_spec(
    seed: u64,
    cell_count: u32,
    geologic_spec: GeologicSpec,
) -> NaturalFixture {
    let mut external = ExternalArtifacts::new();
    external
        .insert(PlanarSpaceArtifact::new(PlanarSpaceSpec {
            width: Meters::new(4_000_000.0).unwrap(),
            height: Meters::new(2_000_000.0).unwrap(),
            target_cell_count: cell_count,
            boundary: BoundaryCondition::Closed,
        }))
        .unwrap();
    external
        .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
        .unwrap();
    external
        .insert(GeologicSpecArtifact::new(geologic_spec))
        .unwrap();
    external
        .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
        .unwrap();
    external
        .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
        .unwrap();
    external
        .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
        .unwrap();
    external
        .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
        .unwrap();
    let outcome = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(RootSeed::new(seed), external, &mut MemoryStageCache::new())
        .unwrap();
    NaturalFixture {
        spatial: outcome.artifacts.get::<SpatialArtifact>().unwrap(),
        tectonic: outcome.artifacts.get::<TectonicArtifact>().unwrap(),
        mantle: outcome.artifacts.get::<MantleArtifact>().unwrap(),
        relief: outcome.artifacts.get::<ReliefArtifact>().unwrap(),
        geology: outcome.artifacts.get::<GeologicArtifact>().unwrap(),
        climate: outcome
            .artifacts
            .get::<PreliminaryClimateArtifact>()
            .unwrap(),
        hydro_erosion: outcome.artifacts.get::<HydroErosionArtifact>().unwrap(),
    }
}

fn assert_plate_connectivity_and_balance(
    seed: u64,
    spatial: &SpatialSnapshot,
    tectonic: &TectonicSnapshot,
) {
    let mut plate_counts = vec![0_usize; tectonic.plates().len()];
    for &plate in tectonic.cell_plates().raw_values() {
        plate_counts[plate as usize] += 1;
    }
    for (plate_index, &expected_count) in plate_counts.iter().enumerate() {
        assert!(expected_count > 0, "seed {seed}: empty plate {plate_index}");
        assert!(
            expected_count as f32 / spatial.cell_count() as f32 <= 0.40,
            "seed {seed}: plate {plate_index} consumed {expected_count} cells"
        );
        let start = tectonic.plates()[plate_index].seed_cell;
        let mut seen = vec![false; spatial.cell_count()];
        let mut queue = VecDeque::from([start]);
        seen[start.raw() as usize] = true;
        let mut connected = 0;
        while let Some(cell) = queue.pop_front() {
            connected += 1;
            for &neighbor in spatial.neighbors(cell).unwrap() {
                let index = neighbor.raw() as usize;
                if !seen[index]
                    && tectonic.cell_plates().raw_values()[index] as usize == plate_index
                {
                    seen[index] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        assert_eq!(
            connected, expected_count,
            "seed {seed}: plate {plate_index} is disconnected"
        );
    }
}

fn assert_boundary_partition_and_motion(
    seed: u64,
    spatial: &SpatialSnapshot,
    tectonic: &TectonicSnapshot,
) {
    let mut member_edges = BTreeSet::new();
    for segment in tectonic.boundary_segments() {
        assert!(!segment.member_edges.is_empty());
        for edge in &segment.member_edges {
            assert!(
                member_edges.insert(*edge),
                "seed {seed}: edge {:?} appears in two segments",
                edge
            );
            let record = &tectonic.boundaries()[edge.raw() as usize];
            assert_eq!(record.segment_id, Some(segment.id));
            assert_eq!(record.kind, segment.kind);
        }
    }

    let mut cross_plate_edges = 0;
    for edge in spatial.edges() {
        let record = &tectonic.boundaries()[edge.id.raw() as usize];
        let [Some(a), Some(b)] = edge.cells else {
            assert_eq!(record.kind, BoundaryKind::None);
            assert_eq!(record.segment_id, None);
            continue;
        };
        let plate_a = tectonic.cell_plates().get(a.raw() as usize).unwrap();
        let plate_b = tectonic.cell_plates().get(b.raw() as usize).unwrap();
        if plate_a == plate_b {
            assert_eq!(record.kind, BoundaryKind::None);
            assert_eq!(record.segment_id, None);
            continue;
        }
        cross_plate_edges += 1;
        assert_ne!(record.kind, BoundaryKind::None);
        assert!(record.segment_id.is_some());
        assert_ne!(
            tectonic.plates()[plate_a.raw() as usize].velocity,
            tectonic.plates()[plate_b.raw() as usize].velocity,
            "seed {seed}: adjacent plates are co-moving"
        );
        assert!(member_edges.contains(&edge.id));
    }
    assert_eq!(member_edges.len(), cross_plate_edges);
}

fn plate_has_mixed_crust(tectonic: &TectonicSnapshot) -> bool {
    let mut kinds = vec![[false; 2]; tectonic.plates().len()];
    for (&plate, &kind) in tectonic
        .cell_plates()
        .raw_values()
        .iter()
        .zip(tectonic.crust_kinds().raw_values())
    {
        kinds[plate as usize][kind as usize] = true;
    }
    kinds.into_iter().any(|present| present[0] && present[1])
}

fn crust_components(spatial: &SpatialSnapshot, tectonic: &TectonicSnapshot) -> Vec<Vec<usize>> {
    let kinds = tectonic.crust_kinds().raw_values();
    let mut visited = vec![false; spatial.cell_count()];
    let mut components = Vec::new();
    for start in 0..spatial.cell_count() {
        if visited[start] {
            continue;
        }
        let kind = kinds[start];
        let mut component = Vec::new();
        let mut queue = VecDeque::from([CellId::from_raw(start as u32)]);
        visited[start] = true;
        while let Some(cell) = queue.pop_front() {
            component.push(cell.raw() as usize);
            for &neighbor in spatial.neighbors(cell).unwrap() {
                let index = neighbor.raw() as usize;
                if !visited[index] && kinds[index] == kind {
                    visited[index] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        components.push(component);
    }
    components
}

fn assert_relief_finite_and_explainable(seed: u64, relief: &ReliefSnapshot) {
    let fields = [
        relief.crust_base_elevation_m().values(),
        relief.tectonic_offset_m().values(),
        relief.volcanic_offset_m().values(),
        relief.regional_offset_m().values(),
        relief.elevation_m().values(),
    ];
    assert!(
        fields
            .iter()
            .flat_map(|values| values.iter())
            .all(|value| value.is_finite()),
        "seed {seed}: non-finite relief value"
    );
    for index in 0..relief.cell_count() as usize {
        let expected = relief.crust_base_elevation_m().values()[index]
            + relief.tectonic_offset_m().values()[index]
            + relief.volcanic_offset_m().values()[index]
            + relief.regional_offset_m().values()[index];
        let actual = relief.elevation_m().values()[index];
        assert!(
            (expected - actual).abs() <= COMPONENT_IDENTITY_TOLERANCE_M,
            "seed {seed}: component identity failed at cell {index}"
        );
    }
}

fn assert_geologic_quality(
    seed: u64,
    tectonic: &TectonicSnapshot,
    mantle: &MantleSnapshot,
    relief: &ReliefSnapshot,
    geology: &GeologicSnapshot,
) {
    assert!(
        mantle.hotspots().iter().any(|hotspot| {
            relief.volcanic_offset_m().values()[hotspot.source_cell().raw() as usize] > 0.0
        }),
        "seed {seed}: a hotspot must produce positive local volcanic relief"
    );
    let heat_min = mantle
        .heat_flow_mw_m2()
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    let heat_max = mantle
        .heat_flow_mw_m2()
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        heat_max - heat_min >= 100.0,
        "seed {seed}: heat-flow anomaly spread was {}",
        heat_max - heat_min
    );

    let categories: BTreeSet<_> = (0..geology.cell_count())
        .map(|index| {
            geology
                .bedrock_kind(CellId::from_raw(index))
                .expect("geologic field is dense")
        })
        .collect();
    assert!(
        categories.contains(&BedrockKind::OceanicMafic),
        "seed {seed}: missing oceanic mafic bedrock"
    );
    assert!(
        categories.contains(&BedrockKind::ContinentalCrystalline),
        "seed {seed}: missing continental crystalline bedrock"
    );
    assert!(
        categories.iter().any(|kind| matches!(
            kind,
            BedrockKind::Volcanic | BedrockKind::Metamorphic | BedrockKind::Sedimentary
        )),
        "seed {seed}: missing active bedrock class"
    );

    let potentials = [
        geology.metallic_mineral_potential(),
        geology.geothermal_potential(),
        geology.sedimentary_basin_potential(),
    ];
    for (name, values) in [
        ("metallic", potentials[0]),
        ("geothermal", potentials[1]),
        ("sedimentary", potentials[2]),
    ] {
        let minimum = values.iter().copied().fold(f32::INFINITY, f32::min);
        let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            maximum - minimum > 0.02,
            "seed {seed}: {name} potential spread was {}",
            maximum - minimum
        );
    }
    assert_ne!(potentials[0], potentials[1]);
    assert_ne!(potentials[0], potentials[2]);
    assert_ne!(potentials[1], potentials[2]);

    let mut sorted_geothermal = geology.geothermal_potential().to_vec();
    sorted_geothermal.sort_by(f32::total_cmp);
    let upper_quartile = sorted_geothermal[sorted_geothermal.len() * 3 / 4];
    let hottest_fractured_source = mantle
        .hotspots()
        .iter()
        .max_by(|first, second| {
            let first_index = first.source_cell().raw() as usize;
            let second_index = second.source_cell().raw() as usize;
            (mantle.heat_flow_mw_m2()[first_index]
                * (0.45 + 0.55 * geology.fracture_intensity()[first_index]),)
                .partial_cmp(&(mantle.heat_flow_mw_m2()[second_index]
                    * (0.45 + 0.55 * geology.fracture_intensity()[second_index]),))
                .unwrap()
        })
        .expect("default geology has hotspots")
        .source_cell()
        .raw() as usize;
    assert!(
        geology.geothermal_potential()[hottest_fractured_source] >= upper_quartile,
        "seed {seed}: high heat plus fracture must rank in the upper geothermal quartile"
    );

    let oceanic_count = tectonic
        .crust_kinds()
        .raw_values()
        .iter()
        .filter(|&&kind| kind == 0)
        .count();
    assert!(oceanic_count > 0 && oceanic_count < tectonic.cell_count() as usize);
}

fn assert_climate_quality(
    seed: u64,
    spatial: &SpatialSnapshot,
    relief: &ReliefSnapshot,
    climate: &PreliminaryClimateSnapshot,
) {
    let northern = climate
        .latitude_degrees()
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .unwrap()
        .0;
    let southern = climate
        .latitude_degrees()
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .unwrap()
        .0;
    assert!(
        climate.monthly_air_temperature_c().values()[northern][5]
            > climate.monthly_air_temperature_c().values()[northern][11],
        "seed {seed}: northern summer phase did not lead northern winter"
    );
    assert!(
        climate.monthly_air_temperature_c().values()[southern][5]
            < climate.monthly_air_temperature_c().values()[southern][11],
        "seed {seed}: southern summer phase did not oppose the north"
    );

    let low_latitude = mean(
        climate
            .mean_annual_air_temperature_c()
            .iter()
            .enumerate()
            .filter(|(cell, _)| climate.latitude_degrees()[*cell].abs() < 15.0)
            .map(|(cell, &temperature)| {
                temperature + relief.elevation_m().values()[cell].max(0.0) * 0.0065
            }),
    );
    let high_latitude = mean(
        climate
            .mean_annual_air_temperature_c()
            .iter()
            .enumerate()
            .filter(|(cell, _)| climate.latitude_degrees()[*cell].abs() > 50.0)
            .map(|(cell, &temperature)| {
                temperature + relief.elevation_m().values()[cell].max(0.0) * 0.0065
            }),
    );
    assert!(
        low_latitude > high_latitude + 10.0,
        "seed {seed}: lapse-adjusted low/high latitude temperatures were {low_latitude}/{high_latitude}"
    );

    let ocean_seasonality = mean(
        climate
            .temperature_seasonality_c()
            .iter()
            .enumerate()
            .filter(|(cell, _)| relief.land_ocean().raw_values()[*cell] == 0)
            .map(|(_, &value)| value),
    );
    let interior_seasonality_values = climate
        .temperature_seasonality_c()
        .iter()
        .enumerate()
        .filter(|(cell, _)| {
            relief.land_ocean().raw_values()[*cell] == 1
                && climate.maritime_influence()[*cell] < 0.35
        })
        .map(|(_, &value)| value)
        .collect::<Vec<_>>();
    if !interior_seasonality_values.is_empty() {
        let interior_seasonality = mean(interior_seasonality_values.into_iter());
        assert!(
            ocean_seasonality < interior_seasonality,
            "seed {seed}: ocean/interior seasonality were {ocean_seasonality}/{interior_seasonality}"
        );
    }

    let land_precipitation = climate
        .annual_precipitation_mm()
        .iter()
        .enumerate()
        .filter(|(cell, _)| relief.land_ocean().raw_values()[*cell] == 1)
        .map(|(_, &value)| value)
        .collect::<Vec<_>>();
    let minimum = land_precipitation
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    let maximum = land_precipitation
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        maximum - minimum > 120.0,
        "seed {seed}: land annual-precipitation spread was {}",
        maximum - minimum
    );
    assert!(
        climate
            .monthly_air_temperature_c()
            .values()
            .iter()
            .flatten()
            .chain(climate.monthly_precipitation_mm().values().iter().flatten())
            .all(|value| value.is_finite()),
        "seed {seed}: non-finite monthly climate scalar"
    );
    assert_eq!(spatial.cell_count(), climate.cell_count() as usize);
}

fn assert_hydro_erosion_quality(
    seed: u64,
    spatial: &SpatialSnapshot,
    snapshot: &HydroErosionSnapshot,
) {
    let surface = snapshot.surface();
    let hydrology = snapshot.hydrology();
    let cell_count = spatial.cell_count();

    for origin in 0..cell_count {
        let mut current = Some(CellId::from_raw(origin as u32));
        let mut steps = 0;
        while let Some(cell) = current {
            steps += 1;
            assert!(
                steps <= cell_count,
                "seed {seed}: receiver chain from cell {origin} did not terminate"
            );
            current = hydrology.flow_receiver()[cell.raw() as usize];
        }
    }
    assert!(
        hydrology
            .drainage_area_km2()
            .iter()
            .all(|&area| area.is_finite() && area > 0.0),
        "seed {seed}: every cell must accumulate a positive finite drainage area"
    );

    let lake_cells = hydrology
        .surface_water()
        .raw_values()
        .iter()
        .filter(|&&kind| kind == SurfaceWaterKind::Lake.raw())
        .count();
    let eroded_cells = surface
        .erosion_depth_m()
        .iter()
        .filter(|&&depth| depth > 0.0)
        .count();
    let deposited_cells = surface
        .deposition_thickness_m()
        .iter()
        .filter(|&&depth| depth > 0.0)
        .count();
    let max_order = hydrology
        .strahler_order()
        .raw_values()
        .iter()
        .copied()
        .max()
        .unwrap_or(0);
    let isolated_eroded = isolated_positive_cells(spatial, surface.erosion_depth_m());
    let isolated_deposited = isolated_positive_cells(spatial, surface.deposition_thickness_m());
    assert!(
        !hydrology.lakes().is_empty() && lake_cells > 0,
        "seed {seed}: expected at least one published lake"
    );
    assert!(
        !hydrology.river_segments().is_empty() && max_order >= 2,
        "seed {seed}: expected a branching published river network"
    );
    assert!(
        eroded_cells > 0,
        "seed {seed}: expected nonzero fluvial erosion"
    );
    assert!(
        deposited_cells > 0
            && surface
                .sediment_throughput_m3()
                .iter()
                .any(|&volume| volume > 0.0)
            && surface.sediment_export_m3() > 0.0,
        "seed {seed}: expected routed, deposited, and exported sediment"
    );
    assert!(
        isolated_eroded <= 2 && isolated_deposited <= 2,
        "seed {seed}: process fill contains too many isolated one-cell speckles \
         (erosion={isolated_eroded}, deposition={isolated_deposited})"
    );
    eprintln!(
        "seed={seed} hydro lakes={} lake_cells={lake_cells} rivers={} max_order={max_order} eroded={eroded_cells} deposited={deposited_cells} isolated_eroded={isolated_eroded} isolated_deposited={isolated_deposited} export_m3={:.3}",
        hydrology.lakes().len(),
        hydrology.river_segments().len(),
        surface.sediment_export_m3(),
    );
}

fn isolated_positive_cells(spatial: &SpatialSnapshot, values: &[f32]) -> usize {
    values
        .iter()
        .enumerate()
        .filter(|&(index, value)| {
            *value > 0.0
                && spatial
                    .neighbors(CellId::from_raw(index as u32))
                    .expect("quality fixture has every cell")
                    .iter()
                    .all(|neighbor| values[neighbor.raw() as usize] == 0.0)
        })
        .count()
}

fn mean(values: impl Iterator<Item = f32>) -> f32 {
    let (sum, count) = values.fold((0.0_f32, 0_usize), |(sum, count), value| {
        (sum + value, count + 1)
    });
    assert!(count > 0);
    sum / count as f32
}

fn golden_packets() -> Vec<(&'static str, PreparedFieldDisplay)> {
    let fixture = build_natural(GOLDEN_SEED, QUALITY_CELL_COUNT);
    let mesh = Arc::new(
        PreparedCellMesh::build(fixture.spatial.snapshot(), MeshCompleteness::RequireAll).unwrap(),
    );
    vec![
        (
            "plate.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                plate_id_field_id(),
                FieldPayloadRef::CategoryU32(
                    fixture.tectonic.snapshot().cell_plates().raw_values(),
                ),
                DisplayRangeMode::Data,
                PaletteId::Categorical,
            ),
        ),
        (
            "crust.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                crust_kind_field_id(),
                FieldPayloadRef::CategoryU32(
                    fixture.tectonic.snapshot().crust_kinds().raw_values(),
                ),
                DisplayRangeMode::Data,
                PaletteId::Categorical,
            ),
        ),
        (
            "elevation.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                elevation_field_id(),
                FieldPayloadRef::ScalarF32(fixture.relief.snapshot().elevation_m().values()),
                symmetric_elevation_range(fixture.relief.snapshot()),
                PaletteId::Diverging,
            ),
        ),
        (
            "current-surface.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                surface_elevation_m_field_id(),
                FieldPayloadRef::ScalarF32(
                    fixture
                        .hydro_erosion
                        .snapshot()
                        .surface()
                        .surface_elevation_m()
                        .values(),
                ),
                symmetric_surface_range(
                    fixture.relief.snapshot(),
                    fixture
                        .hydro_erosion
                        .snapshot()
                        .surface()
                        .surface_elevation_m()
                        .values(),
                ),
                PaletteId::Diverging,
            ),
        ),
        (
            "surface-water.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                surface_water_kind_field_id(),
                FieldPayloadRef::CategoryU32(
                    fixture
                        .hydro_erosion
                        .snapshot()
                        .hydrology()
                        .surface_water()
                        .raw_values(),
                ),
                DisplayRangeMode::Data,
                PaletteId::Categorical,
            ),
        ),
        (
            "strahler-order.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                strahler_stream_order_field_id(),
                FieldPayloadRef::CategoryU32(
                    fixture
                        .hydro_erosion
                        .snapshot()
                        .hydrology()
                        .strahler_order()
                        .raw_values(),
                ),
                DisplayRangeMode::Data,
                PaletteId::Categorical,
            ),
        ),
        (
            "fluvial-erosion-depth.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                fluvial_erosion_depth_m_field_id(),
                FieldPayloadRef::ScalarF32(
                    fixture.hydro_erosion.snapshot().surface().erosion_depth_m(),
                ),
                DisplayRangeMode::Data,
                PaletteId::Sequential,
            ),
        ),
        (
            "sediment-deposition-thickness.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                sediment_deposition_thickness_m_field_id(),
                FieldPayloadRef::ScalarF32(
                    fixture
                        .hydro_erosion
                        .snapshot()
                        .surface()
                        .deposition_thickness_m(),
                ),
                DisplayRangeMode::Data,
                PaletteId::Sequential,
            ),
        ),
        (
            "heat-flow.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                mantle_heat_flow_field_id(),
                FieldPayloadRef::ScalarF32(fixture.mantle.snapshot().heat_flow_mw_m2()),
                DisplayRangeMode::Schema,
                PaletteId::Sequential,
            ),
        ),
        (
            "volcanic-influence.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                volcanic_influence_field_id(),
                FieldPayloadRef::ScalarF32(fixture.mantle.snapshot().volcanic_influence()),
                DisplayRangeMode::Schema,
                PaletteId::Sequential,
            ),
        ),
        (
            "bedrock.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                bedrock_kind_field_id(),
                FieldPayloadRef::CategoryU32(
                    fixture.geology.snapshot().bedrock_kinds().raw_values(),
                ),
                DisplayRangeMode::Data,
                PaletteId::Categorical,
            ),
        ),
        (
            "metallic-potential.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                metallic_mineral_potential_field_id(),
                FieldPayloadRef::ScalarF32(fixture.geology.snapshot().metallic_mineral_potential()),
                DisplayRangeMode::Schema,
                PaletteId::Sequential,
            ),
        ),
        (
            "geothermal-potential.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                geothermal_potential_field_id(),
                FieldPayloadRef::ScalarF32(fixture.geology.snapshot().geothermal_potential()),
                DisplayRangeMode::Schema,
                PaletteId::Sequential,
            ),
        ),
        (
            "sedimentary-basin-potential.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                sedimentary_basin_potential_field_id(),
                FieldPayloadRef::ScalarF32(
                    fixture.geology.snapshot().sedimentary_basin_potential(),
                ),
                DisplayRangeMode::Schema,
                PaletteId::Sequential,
            ),
        ),
        (
            "preliminary-mean-air-temperature.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                preliminary_mean_air_temperature_c_field_id(),
                FieldPayloadRef::ScalarF32(
                    fixture.climate.snapshot().mean_annual_air_temperature_c(),
                ),
                symmetric_zero_range(fixture.climate.snapshot().mean_annual_air_temperature_c()),
                PaletteId::Diverging,
            ),
        ),
        (
            "preliminary-annual-precipitation.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                preliminary_annual_precipitation_mm_field_id(),
                FieldPayloadRef::ScalarF32(fixture.climate.snapshot().annual_precipitation_mm()),
                DisplayRangeMode::Data,
                PaletteId::Sequential,
            ),
        ),
        (
            "maritime-influence.png",
            natural_packet(
                &fixture,
                mesh.clone(),
                maritime_influence_field_id(),
                FieldPayloadRef::ScalarF32(fixture.climate.snapshot().maritime_influence()),
                DisplayRangeMode::Schema,
                PaletteId::Sequential,
            ),
        ),
        (
            "preliminary-temperature-seasonality.png",
            natural_packet(
                &fixture,
                mesh,
                preliminary_temperature_seasonality_c_field_id(),
                FieldPayloadRef::ScalarF32(fixture.climate.snapshot().temperature_seasonality_c()),
                DisplayRangeMode::Data,
                PaletteId::Sequential,
            ),
        ),
    ]
}

fn symmetric_elevation_range(relief: &ReliefSnapshot) -> DisplayRangeMode {
    symmetric_surface_range(relief, relief.elevation_m().values())
}

fn symmetric_surface_range(relief: &ReliefSnapshot, values: &[f32]) -> DisplayRangeMode {
    let sea_level = relief.sea_level_m();
    let radius = values
        .iter()
        .map(|value| (value - sea_level).abs())
        .fold(0.0_f32, f32::max);
    DisplayRangeMode::Manual(ValueRange::new(sea_level - radius, sea_level + radius).unwrap())
}

fn symmetric_zero_range(values: &[f32]) -> DisplayRangeMode {
    let radius = values
        .iter()
        .map(|value| value.abs())
        .fold(0.0_f32, f32::max);
    DisplayRangeMode::Manual(ValueRange::new(-radius, radius).unwrap())
}

fn natural_packet(
    fixture: &NaturalFixture,
    mesh: Arc<PreparedCellMesh>,
    field_id: FieldId,
    payload: FieldPayloadRef<'_>,
    range: DisplayRangeMode,
    palette: PaletteId,
) -> PreparedFieldDisplay {
    let registry =
        natural_field_registry(fixture.tectonic.snapshot().plates().len() as u16).unwrap();
    let catalog = FieldCatalog::from_payloads(&registry, [(field_id.clone(), payload)]).unwrap();
    let view = catalog.get(&field_id).unwrap().view().unwrap();
    let field = Arc::new(prepare_cell_field(view, mesh.cell_count(), range).unwrap());
    let mut clock = DisplayRevisionClock::default();
    PreparedFieldDisplay::new(
        mesh,
        field,
        Arc::new(PreparedDiagnosticMask::empty(
            fixture.spatial.snapshot().cell_count(),
        )),
        Arc::from(built_in_palette(palette)),
        DisplayRevisions::new(
            clock.issue().unwrap(),
            clock.issue().unwrap(),
            clock.issue().unwrap(),
            clock.issue().unwrap(),
        ),
        false,
    )
    .unwrap()
}

fn assert_golden(name: &str, packet: &PreparedFieldDisplay) {
    let actual = rasterize_reference(packet, GOLDEN_WIDTH, GOLDEN_HEIGHT).unwrap();
    let expected = image::ImageReader::open(golden_path(name))
        .unwrap()
        .decode()
        .unwrap()
        .into_rgba8();
    let actual_hash = blake3::hash(actual.rgba8());
    let expected_hash = blake3::hash(expected.as_raw());
    assert_eq!(
        (actual.width(), actual.height()),
        expected.dimensions(),
        "{name}: dimension mismatch; actual={actual_hash}, expected={expected_hash}"
    );
    assert_eq!(
        actual.rgba8(),
        expected.as_raw(),
        "{name}: pixel mismatch; actual={actual_hash}, expected={expected_hash}"
    );
}

fn write_golden(name: &str, packet: &PreparedFieldDisplay) {
    let image = rasterize_reference(packet, GOLDEN_WIDTH, GOLDEN_HEIGHT).unwrap();
    let path = golden_path(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::save_buffer_with_format(
        path,
        image.rgba8(),
        image.width(),
        image.height(),
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .unwrap();
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("natural-foundation")
        .join(name)
}
