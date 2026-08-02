use std::collections::BTreeSet;
use std::sync::OnceLock;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::TectonicGenerator;
use sekai::generators::spatial::PlanarVoronoiBuilder;
use sekai::world::natural::{
    BoundaryKind, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSnapshot,
    TectonicSpec, WorldFormationPreset, MAX_PLATE_VELOCITY_MM_PER_YEAR,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::{SpatialSnapshot, Topology};
use sekai::world::{BoundaryCondition, CellId, Meters, PlanarSpaceSpec, PlateId, RootSeed};

fn spatial_fixture() -> &'static SpatialSnapshot {
    static SPATIAL: OnceLock<SpatialSnapshot> = OnceLock::new();
    SPATIAL.get_or_init(|| {
        PlanarVoronoiBuilder::build(
            &PlanarSpaceSpec {
                width: Meters::new(2_000_000.0).unwrap(),
                height: Meters::new(1_200_000.0).unwrap(),
                target_cell_count: 576,
                boundary: BoundaryCondition::Closed,
            },
            &mut ChaCha8Rng::seed_from_u64(9001),
        )
        .unwrap()
    })
}

fn natural_rng() -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(42),
        StageIdentity::new("natural.tectonics", 2, "sekai.core"),
    ))
}

fn generate(spatial: &SpatialSnapshot) -> TectonicSnapshot {
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap();
    TectonicGenerator::generate(
        spatial,
        &TectonicSpec::default(),
        &formation,
        &mut natural_rng(),
    )
    .unwrap()
}

fn normalized_pair(first: PlateId, second: PlateId) -> [PlateId; 2] {
    if first < second {
        [first, second]
    } else {
        [second, first]
    }
}

#[test]
fn generated_motion_is_bounded_and_adjacent_plates_are_not_co_moving() {
    let spatial = spatial_fixture();
    let snapshot = generate(spatial);
    let mut adjacent_pairs = BTreeSet::new();
    for edge in spatial.edges() {
        let [Some(first), Some(second)] = edge.cells else {
            continue;
        };
        let first = snapshot.plate_for_cell(first).unwrap();
        let second = snapshot.plate_for_cell(second).unwrap();
        if first != second {
            adjacent_pairs.insert(normalized_pair(first, second));
        }
    }

    for plate in snapshot.plates() {
        assert!(plate
            .velocity
            .components_mm_per_year()
            .into_iter()
            .all(|component| component.abs() <= MAX_PLATE_VELOCITY_MM_PER_YEAR));
    }
    for [first, second] in adjacent_pairs {
        let first = snapshot.plates()[first.raw() as usize]
            .velocity
            .components_mm_per_year();
        let second = snapshot.plates()[second.raw() as usize]
            .velocity
            .components_mm_per_year();
        let dx = i32::from(second[0]) - i32::from(first[0]);
        let dy = i32::from(second[1]) - i32::from(first[1]);
        assert!(
            dx * dx + dy * dy >= 24 * 24,
            "moderate activity requires at least 24 mm/year relative motion"
        );
    }
}

#[test]
fn boundary_records_follow_edge_plate_semantics_and_include_strong_events() {
    let spatial = spatial_fixture();
    let snapshot = generate(spatial);
    let mut strong_event_count = 0;
    for edge in spatial.edges() {
        let record = snapshot.boundary_for_edge(edge.id).unwrap();
        match edge.cells {
            [Some(first), Some(second)] => {
                let crosses_plate =
                    snapshot.plate_for_cell(first) != snapshot.plate_for_cell(second);
                if crosses_plate {
                    assert_ne!(record.kind, BoundaryKind::None);
                    assert!(record.segment_id.is_some());
                    strong_event_count += usize::from(record.kind != BoundaryKind::Weak);
                } else {
                    assert_eq!(record.kind, BoundaryKind::None);
                }
            }
            [Some(_), None] | [None, Some(_)] => {
                assert_eq!(record.kind, BoundaryKind::None);
            }
            [None, None] => panic!("validated spatial edge has no owner"),
        }
    }
    assert!(strong_event_count > 0);
}

#[test]
fn compatible_edges_form_stable_continuous_segments() {
    let spatial = spatial_fixture();
    let snapshot = generate(spatial);
    snapshot.validate_against(spatial).unwrap();

    assert!(snapshot
        .boundary_segments()
        .iter()
        .any(|segment| segment.member_edges.len() > 1));
    for (index, segment) in snapshot.boundary_segments().iter().enumerate() {
        assert_eq!(segment.id.raw() as usize, index);
        assert!(segment.plates[0] < segment.plates[1]);
        assert!(!segment.member_edges.is_empty());
        assert!(segment
            .member_edges
            .windows(2)
            .all(|pair| pair[0] < pair[1]));
        for &edge in &segment.member_edges {
            let record = snapshot.boundary_for_edge(edge).unwrap();
            assert_eq!(record.kind, segment.kind);
            assert_eq!(record.segment_id, Some(segment.id));
            assert_eq!(record.subducting_plate, segment.subducting_plate);
        }
    }
}

#[test]
fn spatial_constructor_edge_normalization_cannot_change_boundaries() {
    let spatial = spatial_fixture();
    let cells = (0..spatial.cell_count())
        .map(|index| {
            spatial
                .cell(CellId::from_raw(index as u32))
                .unwrap()
                .clone()
        })
        .collect();
    let mut reversed_edges = spatial.edges().to_vec();
    reversed_edges.reverse();
    let normalized = SpatialSnapshot::new(
        spatial.schema_version,
        spatial.bounds(),
        BoundaryCondition::Closed,
        cells,
        reversed_edges,
    )
    .unwrap();

    assert_eq!(
        serde_json::to_vec(&generate(spatial)).unwrap(),
        serde_json::to_vec(&generate(&normalized)).unwrap()
    );
}
